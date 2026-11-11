#include "ui/TasksPanel.h"

#include "ui/LogView.h"
#include "ui/Tasks.h"

#include <QHBoxLayout>
#include <QLabel>
#include <QListWidget>
#include <QPushButton>
#include <QSettings>
#include <QSplitter>
#include <QStyle>
#include <QToolButton>
#include <QVBoxLayout>

namespace
{

	enum
	{
		TaskRole = Qt::UserRole
	};

	QIcon stateIcon(const QWidget *widget, Task::State state)
	{
		switch (state)
		{
		case Task::State::Running:
			return QIcon::fromTheme("view-refresh", widget->style()->standardIcon(QStyle::SP_BrowserReload));
		case Task::State::Succeeded:
			return QIcon::fromTheme("dialog-ok-apply", widget->style()->standardIcon(QStyle::SP_DialogApplyButton));
		case Task::State::Failed:
			return QIcon::fromTheme("dialog-error", widget->style()->standardIcon(QStyle::SP_MessageBoxCritical));
		case Task::State::Cancelled:
			return QIcon::fromTheme("process-stop", widget->style()->standardIcon(QStyle::SP_BrowserStop));
		}
		return {};
	}

} // namespace

TasksPanel::TasksPanel(TaskManager *tasks, QWidget *parent)
	: QWidget(parent), m_tasks(tasks), m_toggle(new QToolButton(this)), m_summary(new QLabel(this)),
	  m_cancel(new QPushButton(tr("Cancel"), this)), m_clear(new QPushButton(tr("Clear Finished"), this)),
	  m_body(new QWidget(this)), m_list(new QListWidget(this)), m_log(new LogView(this))
{
	m_toggle->setText(tr("Tasks"));
	m_toggle->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
	m_toggle->setAutoRaise(true);
	m_toggle->setCheckable(true);
	m_summary->setTextFormat(Qt::PlainText);

	m_list->setMinimumWidth(180);
	m_list->setUniformItemSizes(true);

	auto *header = new QHBoxLayout;
	header->setContentsMargins(4, 0, 4, 0);
	header->addWidget(m_toggle);
	header->addWidget(m_summary, 1);
	header->addWidget(m_cancel);
	header->addWidget(m_clear);

	auto *split = new QSplitter(Qt::Horizontal, m_body);
	split->addWidget(m_list);
	split->addWidget(m_log);
	split->setStretchFactor(0, 1);
	split->setStretchFactor(1, 3);
	auto *bodyLayout = new QVBoxLayout(m_body);
	bodyLayout->setContentsMargins(0, 0, 0, 0);
	bodyLayout->addWidget(split);

	auto *layout = new QVBoxLayout(this);
	layout->setContentsMargins(0, 2, 0, 0);
	layout->setSpacing(2);
	layout->addLayout(header);
	layout->addWidget(m_body, 1);

	connect(m_toggle, &QToolButton::toggled, this, &TasksPanel::setExpanded);
	connect(m_cancel, &QPushButton::clicked, this, [this]
			{
		if (Task *task = selectedTask())
			task->cancel(); });
	connect(m_clear, &QPushButton::clicked, m_tasks, &TaskManager::clearFinished);
	connect(m_list, &QListWidget::currentItemChanged, this, &TasksPanel::showSelected);
	connect(m_tasks, &TaskManager::taskAdded, this, &TasksPanel::addTask);
	connect(m_tasks, &TaskManager::taskRemoved, this, &TasksPanel::removeTask);
	connect(m_tasks, &TaskManager::countsChanged, this, &TasksPanel::updateSummary);

	setExpanded(QSettings().value("tasks/expanded", true).toBool());
	updateSummary();
	showSelected();
}

void TasksPanel::setExpanded(bool expanded)
{
	m_expanded = expanded;
	m_body->setVisible(expanded);
	m_toggle->setChecked(expanded);
	m_toggle->setArrowType(expanded ? Qt::DownArrow : Qt::UpArrow);
	QSettings().setValue("tasks/expanded", expanded);
	emit expandedChanged(expanded);
}

void TasksPanel::addTask(Task *task)
{
	auto *item = new QListWidgetItem;
	item->setData(TaskRole, QVariant::fromValue(static_cast<QObject *>(task)));
	m_list->insertItem(0, item);
	connect(task, &Task::stateChanged, this, [this, task]
			{
		updateItem(task);
		if (task == selectedTask())
			m_cancel->setEnabled(false); });
	connect(task, &Task::progressChanged, this, [this, task]
			{ updateItem(task); });
	connect(task, &Task::lineAdded, this, [this, task](const QString &line)
			{
		if (task == selectedTask())
			m_log->appendLine(line); });
	updateItem(task);
	m_list->setCurrentItem(item);
	if (!isExpanded())
		setExpanded(true);
}

void TasksPanel::removeTask(Task *task)
{
	delete itemFor(task);
	if (m_list->count() == 0)
		showSelected();
}

void TasksPanel::updateItem(Task *task)
{
	QListWidgetItem *item = itemFor(task);
	if (!item)
		return;
	QString text = task->title();
	if (task->isRunning() && task->progress() >= 0)
		text += QStringLiteral(" — %1%").arg(task->progress());
	item->setText(text);
	item->setIcon(stateIcon(this, task->state()));
	item->setToolTip(task->title());
}

void TasksPanel::showSelected()
{
	m_log->clear();
	Task *task = selectedTask();
	m_cancel->setEnabled(task && task->isRunning());
	if (!task)
		return;
	for (const QString &line : task->lines())
		m_log->appendLine(line);
}

void TasksPanel::updateSummary()
{
	const int running = m_tasks->runningCount();
	m_summary->setText(running > 0 ? tr("%n running", nullptr, running) : QString());
	m_clear->setEnabled(m_tasks->tasks().size() > running);
}

QListWidgetItem *TasksPanel::itemFor(Task *task) const
{
	for (int i = 0; i < m_list->count(); ++i)
	{
		if (m_list->item(i)->data(TaskRole).value<QObject *>() == task)
			return m_list->item(i);
	}
	return nullptr;
}

Task *TasksPanel::selectedTask() const
{
	QListWidgetItem *item = m_list->currentItem();
	return item ? static_cast<Task *>(item->data(TaskRole).value<QObject *>()) : nullptr;
}
