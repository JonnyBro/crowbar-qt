#include "ui/GameList.h"

#include "core/Launcher.h"

#include <QDir>
#include <QFileInfo>

#include <QSettings>

#include <algorithm>
#include <QStandardPaths>

void GameList::refresh()
{
	const QList<steam::Installation> installations = steam::scanAll();
	m_steamFound = !installations.isEmpty();
	m_apps.clear();
	m_libraries.clear();
	m_compatTools.clear();

	for (const steam::Installation &installation : installations)
	{
		m_libraries += installation.libraries;
		m_compatTools += installation.compatTools;
		for (steam::App app : installation.apps)
		{
			if (!app.isSourceGame())
				continue;
			const QString tools = toolsOverride(app.id);
			if (!tools.isEmpty())
			{
				// One prefix per game keeps its Wine settings apart from the others.
				const QString prefix = QStringLiteral("%1/prefixes/%2")
										   .arg(QStandardPaths::writableLocation(QStandardPaths::AppDataLocation))
										   .arg(app.id);
				steam::overrideTools(app, tools, installation.compatTools, prefix);
			}
			m_apps << app;
		}
	}

	// Added by hand after the scan, so they borrow tools that were overridden above.
	const QList<steam::App> installed = m_apps;
	for (const QString &folder : customFolders())
	{
		const QString cleaned = QDir::cleanPath(folder);
		const bool listed = std::any_of(installed.begin(), installed.end(), [&](const steam::App &app)
										{ return app.gameDirs.contains(cleaned); });
		if (listed)
			continue;
		if (auto app = steam::appForGameDir(cleaned, installed))
		{
			if (app->steamRoot.isEmpty() && !installations.isEmpty())
				app->steamRoot = installations.first().root;
			m_apps << *app;
		}
	}
	std::sort(m_apps.begin(), m_apps.end(), [](const steam::App &a, const steam::App &b)
			  { return a.name.localeAwareCompare(b.name) < 0; });

	// A Proton chosen on the settings page runs the tools of every game that uses Proton.
	const QString proton = QSettings().value("tools/proton").toString();
	const auto chosen = std::find_if(m_compatTools.begin(), m_compatTools.end(), [&](const steam::CompatTool &tool)
									 { return tool.name == proton; });
	if (chosen != m_compatTools.end())
	{
		for (steam::App &app : m_apps)
		{
			if (app.protonPath.isEmpty())
				continue;
			app.protonPath = chosen->path;
			app.compatToolName = chosen->name;
		}
	}

	// Keep the chosen game across refreshes and restarts; fall back to the first one.
	const QList<Target> all = targets();
	if (m_currentDir.isEmpty())
	{
		const QString saved = QSettings().value("game/current").toString();
		// Older versions saved "appid:folder name" instead of the full path.
		const qsizetype colon = saved.indexOf(u':');
		if (!saved.startsWith(u'/') && colon > 0)
		{
			const quint32 id = saved.left(colon).toUInt();
			const QString name = saved.mid(colon + 1);
			for (const Target &target : all)
			{
				if (m_currentDir.isEmpty() && target.app.id == id && QFileInfo(target.gameDir).fileName() == name)
					m_currentDir = target.gameDir;
			}
		}
		else
		{
			m_currentDir = saved;
		}
	}
	const bool known = std::any_of(all.begin(), all.end(), [this](const Target &target)
								   { return target.gameDir == m_currentDir; });
	if (!known)
	{
		// Prefer a game that can actually run the tools.
		const auto withTools = std::find_if(all.begin(), all.end(), [](const Target &target)
											{ return !target.app.toolsPath.isEmpty(); });
		const auto chosen = withTools != all.end() ? withTools : all.begin();
		m_currentDir = chosen != all.end() ? chosen->gameDir : QString();
	}
	emit changed();
	emit currentChanged();
}

QStringList GameList::customFolders()
{
	return QSettings().value("games/custom").toStringList();
}

void GameList::addCustomFolder(const QString &gameDir)
{
	QStringList folders = customFolders();
	const QString cleaned = QDir::cleanPath(gameDir);
	if (!folders.contains(cleaned))
		folders << cleaned;
	QSettings().setValue("games/custom", folders);
}

void GameList::removeCustomFolder(const QString &gameDir)
{
	QStringList folders = customFolders();
	folders.removeAll(QDir::cleanPath(gameDir));
	QSettings().setValue("games/custom", folders);
}

QString GameList::Target::tool(const QString &name) const
{
	return launcher::findTool(app, name);
}

QList<GameList::Target> GameList::targets() const
{
	QList<Target> result;
	for (const steam::App &app : m_apps)
	{
		for (const QString &gameDir : app.gameDirs)
			result.append({app, gameDir});
	}
	return result;
}

std::optional<GameList::Target> GameList::current() const
{
	for (const Target &target : targets())
	{
		if (target.gameDir == m_currentDir)
			return target;
	}
	return std::nullopt;
}

void GameList::setCurrent(const QString &gameDir)
{
	const QString dir = QDir::cleanPath(gameDir);
	if (dir == m_currentDir)
		return;
	m_currentDir = dir;
	QSettings().setValue("game/current", dir);
	emit currentChanged();
}

void GameList::setToolsOverride(quint32 appId, const QString &toolsPath)
{
	const QString key = QStringLiteral("games/%1/tools").arg(appId);
	if (toolsPath.isEmpty())
		QSettings().remove(key);
	else
		QSettings().setValue(key, toolsPath);
}

QString GameList::toolsOverride(quint32 appId)
{
	return QSettings().value(QStringLiteral("games/%1/tools").arg(appId)).toString();
}
