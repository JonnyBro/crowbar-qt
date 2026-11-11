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

// Looks for <name>.exe or <name> in the game's bin folder, preferring a
// 64-bit build from its win64, x64 or linux64 subfolder.
QString findTool(const steam::App &app, const QString &name);

QString toWindowsPath(const QString &hostPath);

// Rewrites every Z:\... path in tool output back to a host path.
QString toHostPaths(QString text);

bool isProtonNoise(QStringView line);

Command build(const steam::App &app, const QString &executable, const QStringList &arguments, Mode mode);

} // namespace launcher
