#pragma once

#include "core/Steam.h"

#include <QProcessEnvironment>
#include <QString>
#include <QStringList>

namespace launcher {

enum class Mode {
    Console, // output is captured
    Gui,     // needs the Steam client, output is not needed
};

struct Command {
    QString program;
    QStringList arguments;
    QProcessEnvironment environment;
    QString workingDirectory;
    QString error;

    bool isValid() const { return error.isEmpty() && !program.isEmpty(); }
    QString displayString() const;
};

bool isWindowsExecutable(const QString &path);

QString toWindowsPath(const QString &hostPath);

// Rewrites every Z:\... path in tool output back to a host path.
QString toHostPaths(QString text);

bool isProtonNoise(QStringView line);

Command build(const steam::App &app, const QString &executable, const QStringList &arguments, Mode mode);

} // namespace launcher
