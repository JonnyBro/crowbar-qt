#pragma once

#include "core/Launcher.h"

#include <QList>
#include <QObject>
#include <QStringList>

#include <functional>

class ToolProcess;

// One run of something: a tool, an unpack, a download. Owns its log.
class Task : public QObject {
    Q_OBJECT

public:
    enum class State { Running, Succeeded, Failed, Cancelled };

    explicit Task(const QString &title, QObject *parent = nullptr);

    QString title() const { return m_title; }
    State state() const { return m_state; }
    bool isRunning() const { return m_state == State::Running; }
    const QStringList &lines() const { return m_lines; }
    // -1 while unknown, otherwise 0..100.
    int progress() const { return m_progress; }

    void appendLine(const QString &line);
    void setProgress(int percent);
    void finish(State state);
    void fail(const QString &error);

    // Runs a tool and logs its output. `onFinished` may do follow-up work and
    // returns whether the task succeeded; without it the exit code decides.
    void run(const launcher::Command &command, std::function<bool(int exitCode, bool success)> onFinished = {});

    // For work not started through run(), e.g. a thread.
    void setCanceller(std::function<void()> canceller) { m_canceller = std::move(canceller); }
    void cancel();

signals:
    void lineAdded(const QString &line);
    void stateChanged();
    void progressChanged();

private:
    QString m_title;
    State m_state = State::Running;
    QStringList m_lines;
    int m_progress = -1;
    bool m_cancelRequested = false;
    std::function<void()> m_canceller;
};

class TaskManager : public QObject {
    Q_OBJECT

public:
    using QObject::QObject;

    Task *start(const QString &title);
    const QList<Task *> &tasks() const { return m_tasks; }
    int runningCount() const;
    void clearFinished();

signals:
    void taskAdded(Task *task);
    void taskRemoved(Task *task);
    void countsChanged();

private:
    QList<Task *> m_tasks;
};
