#pragma once

#include "core/Steam.h"

#include <QProcessEnvironment>
#include <QString>
#include <QStringList>

namespace launcher
{

	enum class Mode
	{
		Console, // output is captured
		Gui,	 // needs the Steam client, output is not needed
	};

	struct Command
	{
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

	// Windows tools print and accept paths in any case; this finds the real one on
	// disk, e.g. a lowercased "/home/user/my addon/a.qc" from studiomdl's output.
	// `relative` is resolved under `base`; an empty `base` means `relative` is absolute.
	// Returns an empty string if no such file or folder exists.
	QString findPathIgnoringCase(const QString &base, const QString &relative);

	// Rewrites every Z:\... path in tool output back to a host path.
	QString toHostPaths(QString text);

	bool isProtonNoise(QStringView line);

	// Drops terminal colour and cursor codes that some tools (steamcmd) print.
	QString stripTerminalCodes(QString text);

	Command build(const steam::App &app, const QString &executable, const QStringList &arguments, Mode mode);

	// Extra environment for every tool started through build(), e.g. from the
	// settings page. Each line is "NAME=value" to set or "-NAME" to remove;
	// blank lines and lines starting with # are ignored.
	void setToolEnvironment(const QStringList &lines);
	// Applies the lines above to an environment; returns lines it could not read.
	QStringList applyEnvironmentLines(QProcessEnvironment &environment, const QStringList &lines);

} // namespace launcher
