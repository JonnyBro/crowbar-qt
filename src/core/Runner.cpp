#include "core/Runner.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QStandardPaths>

namespace runner
{

	launcher::Command gameCommand(const steam::App &app, const QString &gameDir, const QStringList &options)
	{
		launcher::Command command;
		command.environment = QProcessEnvironment::systemEnvironment();

		// A relative -game works for both builds; an absolute one would need
		// Z:\ form under Proton and a host path natively.
		QStringList arguments{"-applaunch", QString::number(app.id)};
		if (!gameDir.isEmpty() && app.isMod)
			arguments << "-game" << (app.platform == steam::Platform::Windows ? launcher::toWindowsPath(gameDir) : QDir::cleanPath(gameDir));
		else if (!gameDir.isEmpty())
			arguments << "-game" << QDir(app.installPath).relativeFilePath(gameDir);
		arguments += options;

		if (app.steamRoot.contains("/.var/app/com.valvesoftware.Steam/"))
		{
			command.program = QStandardPaths::findExecutable("flatpak");
			command.arguments = QStringList{"run", "com.valvesoftware.Steam"} + arguments;
		}
		else
		{
			command.program = QStandardPaths::findExecutable("steam");
			command.arguments = arguments;
		}
		if (command.program.isEmpty())
			command.error = QCoreApplication::translate("Crowbar", "The steam command was not found.");
		return command;
	}

	launcher::Command hammerCommand(const steam::App &app, const QString &gameDir)
	{
		const QString executable = launcher::findTool(app, "hammer");
		if (executable.isEmpty())
		{
			launcher::Command command;
			command.error = QCoreApplication::translate("Crowbar", "%1 has no Hammer in its bin folder.").arg(app.name);
			return command;
		}
		const bool windows = launcher::isWindowsExecutable(executable);
		launcher::Command command = launcher::build(
			app, executable, {"-nop4", "-game", windows ? launcher::toWindowsPath(gameDir) : gameDir}, launcher::Mode::Gui);
		command.workingDirectory = QFileInfo(executable).absolutePath();
		return command;
	}

} // namespace runner
