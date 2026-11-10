#include "core/Hlmv.h"

#include <QCoreApplication>
#include <QFileInfo>

namespace hlmv
{

	QString findExecutable(const steam::App &app)
	{
		return launcher::findTool(app, "hlmv");
	}

	launcher::Command viewCommand(const steam::App &app, const QString &gameDir, const QString &mdlFile)
	{
		launcher::Command command;
		const QString executable = findExecutable(app);
		if (executable.isEmpty())
		{
			command.error = QCoreApplication::translate("Crowbar", "%1 has no hlmv in its bin folder.").arg(app.name);
			return command;
		}
		if (!mdlFile.isEmpty() && !QFileInfo(mdlFile).isFile())
		{
			command.error = QCoreApplication::translate("Crowbar", "\"%1\" does not exist.").arg(mdlFile);
			return command;
		}

		const bool windows = launcher::isWindowsExecutable(executable);
		const auto path = [&](const QString &hostPath)
		{
			return windows ? launcher::toWindowsPath(hostPath) : hostPath;
		};

		// -olddialogs makes HLMV accept a model from anywhere on disk, not only from the game's folders.
		QStringList arguments{"-olddialogs", "-game", path(gameDir)};
		if (!mdlFile.isEmpty())
			arguments << path(mdlFile);

		command = launcher::build(app, executable, arguments, launcher::Mode::Gui);
		command.workingDirectory = QFileInfo(executable).absolutePath();
		return command;
	}

} // namespace hlmv
