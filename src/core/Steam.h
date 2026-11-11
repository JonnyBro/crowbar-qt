#pragma once

#include <QList>
#include <QString>
#include <QStringList>

#include <optional>

namespace steam
{

	enum class Platform
	{
		Linux,
		Windows
	};

	struct CompatTool
	{
		QString name;
		QString displayName;
		QString path;
	};

	struct App
	{
		quint32 id = 0;
		QString name;
		QString installPath;
		QString steamRoot;
		Platform platform = Platform::Linux;
		QStringList gameDirs;
		QString toolsPath;
		QString compatDataPath;
		QString compatToolName;
		QString protonPath;
		// toolsPath was set by the user rather than found in the game folder.
		bool toolsOverridden = false;
		// A game folder that runs on top of another app (a Source mod, or a folder
		// added by hand); id is then that base app's, and installPath the folder itself.
		bool isMod = false;
		QString baseName;

		bool isSourceGame() const { return !gameDirs.isEmpty(); }
	};

	struct Installation
	{
		QString root;
		QStringList libraries;
		QList<CompatTool> compatTools;
		QList<App> apps;
	};

	QStringList findRoots();
	Installation scan(const QString &root);
	QList<Installation> scanAll();

	// Internal name Steam uses in CompatToolMapping for a Valve-shipped Proton
	// directory, e.g. "Proton 9.0 (Beta)" -> "proton_9". Empty if not recognised.
	QString officialToolName(const QString &directoryName);

	// Builds an app for a folder with a gameinfo.txt, borrowing tools, platform
	// and Proton from the installed app its SteamAppId names. Empty if the folder
	// has no gameinfo.txt.
	std::optional<App> appForGameDir(const QString &gameDir, const QList<App> &installed);

	// Points the app at a tools folder chosen by the user, e.g. the bin folder of
	// the Windows build for a native game. If those tools are Windows programs and
	// the app has no Proton of its own, one of `tools` is used with `prefixDir`.
	void overrideTools(App &app, const QString &toolsPath, const QList<CompatTool> &tools, const QString &prefixDir);

} // namespace steam
