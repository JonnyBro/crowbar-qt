#include "ui/GameList.h"

#include <QSettings>
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

	emit changed();
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
