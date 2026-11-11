#include "core/ToolProcess.h"

#include <QDir>
#include <QFile>
#include <QProcess>
#include <QTimer>
#include <QUuid>

#include <signal.h>

ToolProcess::ToolProcess(QObject *parent)
	: QObject(parent)
{
}

ToolProcess::~ToolProcess()
{
	if (isRunning())
	{
		m_process->disconnect(this);
		killGroup(SIGKILL);
		m_process->waitForFinished(1000);
	}
}

bool ToolProcess::isRunning() const
{
	return m_process && m_process->state() != QProcess::NotRunning;
}

void ToolProcess::start(const launcher::Command &command)
{
	if (isRunning())
		return;
	if (!command.isValid())
	{
		emit failedToStart(command.error);
		return;
	}

	delete m_process;
	m_buffer.clear();
	m_process = new QProcess(this);
	m_process->setProgram(command.program);
	m_process->setArguments(command.arguments);
	// Wine starts Windows programs in sessions of their own, so signalling our
	// process group misses them. They do inherit the environment, which lets
	// killGroup() find them again by this marker.
	const QString runId = QUuid::createUuid().toString(QUuid::WithoutBraces);
	m_marker = "CROWBAR_QT_RUN=" + runId.toLatin1();
	QProcessEnvironment environment = command.environment;
	environment.insert("CROWBAR_QT_RUN", runId);
	m_process->setProcessEnvironment(environment);
	m_process->setWorkingDirectory(command.workingDirectory);
	m_process->setProcessChannelMode(QProcess::MergedChannels);
	m_process->setStandardInputFile(QProcess::nullDevice());
	// Own session so cancel() can signal Proton and every wine child at once.
	m_process->setUnixProcessParameters(QProcess::UnixProcessFlag::CreateNewSession);

	connect(m_process, &QProcess::readyRead, this, [this]
			{ readOutput(false); });
	connect(m_process, &QProcess::finished, this, [this](int exitCode, QProcess::ExitStatus status)
			{
        readOutput(true);
        emit finished(exitCode, status == QProcess::NormalExit && exitCode == 0); });
	connect(m_process, &QProcess::errorOccurred, this, [this](QProcess::ProcessError error)
			{
        if (error == QProcess::FailedToStart)
            emit failedToStart(m_process->errorString()); });
	m_process->start();
}

void ToolProcess::cancel()
{
	if (!isRunning())
		return;
	killGroup(SIGTERM);
	QTimer::singleShot(2000, m_process, [this]
					   {
        if (isRunning())
            killGroup(SIGKILL); });
}

void ToolProcess::killGroup(int signal)
{
	const qint64 pid = m_process->processId();
	if (pid > 0)
		::kill(-pid_t(pid), signal);

	const QDir proc("/proc");
	for (const QString &entry : proc.entryList(QDir::Dirs | QDir::NoDotAndDotDot))
	{
		bool isPid = false;
		const pid_t other = entry.toInt(&isPid);
		if (!isPid)
			continue;
		QFile environ(proc.filePath(entry + "/environ"));
		if (!environ.open(QIODevice::ReadOnly))
			continue;
		if (environ.readAll().split('\0').contains(m_marker))
			::kill(other, signal);
	}
}

void ToolProcess::readOutput(bool flush)
{
	m_buffer += m_process->readAll();
	for (;;)
	{
		qsizetype end = m_buffer.indexOf('\n');
		if (end < 0)
		{
			if (!flush || m_buffer.isEmpty())
				return;
			end = m_buffer.size();
		}
		QString line = launcher::stripTerminalCodes(QString::fromUtf8(m_buffer.left(end)));
		m_buffer.remove(0, end + 1);
		if (line.endsWith(u'\r'))
			line.chop(1);
		if (!launcher::isProtonNoise(line))
			emit outputLine(launcher::toHostPaths(line));
	}
}
