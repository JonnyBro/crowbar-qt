#pragma once

#include "core/Launcher.h"

#include <QObject>

class QProcess;

class ToolProcess : public QObject
{
	Q_OBJECT

public:
	explicit ToolProcess(QObject *parent = nullptr);
	~ToolProcess() override;

	bool isRunning() const;
	void start(const launcher::Command &command);
	void cancel();

signals:
	void outputLine(const QString &line);
	void finished(int exitCode, bool success);
	void failedToStart(const QString &error);

private:
	void readOutput(bool flush);
	void killGroup(int signal);

	QProcess *m_process = nullptr;
	QByteArray m_buffer;
	QByteArray m_marker;
};
