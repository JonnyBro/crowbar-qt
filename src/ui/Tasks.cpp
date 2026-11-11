#include "ui/Tasks.h"

#include "core/ToolProcess.h"

#include <QCoreApplication>

#include <algorithm>

namespace
{

	// Finished tasks beyond this are dropped, oldest first.
	constexpr int MaxFinishedTasks = 50;

} // namespace

Task::Task(const QString &title, QObject *parent)
	: QObject(parent), m_title(title)
{
}

void Task::appendLine(const QString &line)
{
	m_lines << line;
	emit lineAdded(line);
}

void Task::setProgress(int percent)
{
	if (percent == m_progress)
		return;
	m_progress = percent;
	emit progressChanged();
}

void Task::finish(State state)
{
	if (m_state != State::Running)
		return;
	m_state = m_cancelRequested && state != State::Succeeded ? State::Cancelled : state;
	m_canceller = {};
	emit stateChanged();
}

void Task::fail(const QString &error)
{
	appendLine(QCoreApplication::translate("Crowbar", "ERROR: %1").arg(error));
	finish(State::Failed);
}

void Task::run(const launcher::Command &command, std::function<bool(int, bool)> onFinished)
{
	if (!command.isValid())
	{
		fail(command.error);
		return;
	}
	appendLine(command.displayString());
	appendLine({});

	auto *process = new ToolProcess(this);
	connect(process, &ToolProcess::outputLine, this, &Task::appendLine);
	connect(process, &ToolProcess::failedToStart, this, &Task::fail);
	connect(process, &ToolProcess::finished, this, [this, onFinished](int exitCode, bool success)
			{
		const bool ok = onFinished ? onFinished(exitCode, success) : success;
		if (!ok && !m_cancelRequested && !onFinished)
			appendLine(QCoreApplication::translate("Crowbar", "ERROR: Exited with code %1.").arg(exitCode));
		finish(ok ? State::Succeeded : State::Failed); });
	m_canceller = [process]
	{ process->cancel(); };
	process->start(command);
}

void Task::cancel()
{
	if (!isRunning() || !m_canceller)
		return;
	m_cancelRequested = true;
	appendLine(QCoreApplication::translate("Crowbar", "Cancelling…"));
	m_canceller();
}

Task *TaskManager::start(const QString &title)
{
	auto *task = new Task(title, this);
	connect(task, &Task::stateChanged, this, [this]
			{
		// Keep the list short, but never drop a task that is still running.
		int finished = 0;
		for (int i = int(m_tasks.size()) - 1; i >= 0; --i)
		{
			Task *candidate = m_tasks[i];
			if (candidate->isRunning() || ++finished <= MaxFinishedTasks)
				continue;
			m_tasks.removeAt(i);
			emit taskRemoved(candidate);
			candidate->deleteLater();
		}
		emit countsChanged(); });
	m_tasks << task;
	emit taskAdded(task);
	emit countsChanged();
	return task;
}

int TaskManager::runningCount() const
{
	return int(std::count_if(m_tasks.begin(), m_tasks.end(), [](const Task *task)
							 { return task->isRunning(); }));
}

void TaskManager::clearFinished()
{
	for (int i = int(m_tasks.size()) - 1; i >= 0; --i)
	{
		Task *task = m_tasks[i];
		if (task->isRunning())
			continue;
		m_tasks.removeAt(i);
		emit taskRemoved(task);
		task->deleteLater();
	}
	emit countsChanged();
}
