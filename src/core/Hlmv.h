#pragma once

#include "core/Launcher.h"

namespace hlmv
{

	QString findExecutable(const steam::App &app);

	// mdlFile may be empty to open the viewer without a model.
	launcher::Command viewCommand(const steam::App &app, const QString &gameDir, const QString &mdlFile);

} // namespace hlmv
