#pragma once

#include "core/Launcher.h"

namespace studiomdl {

struct Options {
    bool noP4 = true;
    bool verbose = false;
    QStringList extra;
};

QString findExecutable(const steam::App &app);

launcher::Command compileCommand(const steam::App &app, const QString &gameDir,
                                 const QString &qcFile, const Options &options);

// Value of $modelname, with forward slashes and a .mdl extension. Empty if absent.
QString readModelName(const QString &qcFile);

// studiomdl can only write into <gameDir>/models, so a different output folder
// is done the way Crowbar does it: move the compiled files afterwards, keeping
// the models/<subpath> layout. Returns false if nothing could be moved.
bool moveCompiledModel(const QString &gameDir, const QString &modelName,
                       const QString &outputDir, QStringList &log);

} // namespace studiomdl
