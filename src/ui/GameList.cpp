#include "ui/GameList.h"

#include "core/Launcher.h"

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

	for (const steam::Installation &installation : installations)
	{
		m_libraries += installation.libraries;
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

	// Keep the chosen game across refreshes and restarts; fall back to the first one.
	if (m_currentApp == 0)
	{
		const QStringList saved = QSettings().value("game/current").toString().split(u':');
		if (saved.size() == 2)
		{
			m_currentApp = saved[0].toUInt();
			m_currentDir = saved[1];
		}
	}
	const QList<Target> all = targets();
	const bool known = std::any_of(all.begin(), all.end(), [this](const Target &target)
								   { return target.app.id == m_currentApp && QFileInfo(target.gameDir).fileName() == m_currentDir; });
	if (!known)
	{
		// Prefer a game that can actually run the tools.
		const auto withTools = std::find_if(all.begin(), all.end(), [](const Target &target)
											{ return !target.app.toolsPath.isEmpty(); });
		const auto chosen = withTools != all.end() ? withTools : all.begin();
		m_currentApp = chosen != all.end() ? chosen->app.id : 0;
		m_currentDir = chosen != all.end() ? QFileInfo(chosen->gameDir).fileName() : QString();
	}
	emit changed();
	emit currentChanged();
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
		if (target.app.id == m_currentApp && QFileInfo(target.gameDir).fileName() == m_currentDir)
			return target;
	}
	return std::nullopt;
}

void GameList::setCurrent(quint32 appId, const QString &gameDir)
{
	const QString dir = QFileInfo(gameDir).fileName();
	if (appId == m_currentApp && dir == m_currentDir)
		return;
	m_currentApp = appId;
	m_currentDir = dir;
	QSettings().setValue("game/current", QStringLiteral("%1:%2").arg(appId).arg(dir));
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
