#pragma once

#include "core/Launcher.h"

namespace runner {

// Asks the Steam client to start the game, so Steam picks Proton or the native
// build itself. `gameDir` becomes -game, relative to the install folder.
launcher::Command gameCommand(const steam::App &app, const QString &gameDir, const QStringList &options);

launcher::Command hammerCommand(const steam::App &app, const QString &gameDir);

} // namespace runner
