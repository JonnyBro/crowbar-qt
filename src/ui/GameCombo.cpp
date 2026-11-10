#include "ui/GameCombo.h"

#include "core/Launcher.h"
#include "ui/GameList.h"

#include <QFileInfo>
#include <QSettings>
#include <QStandardItemModel>

namespace
{

	QString targetKey(const steam::App &app, const QString &gameDir)
	{
		return QStringLiteral("%1:%2").arg(app.id).arg(QFileInfo(gameDir).fileName());
	}

} // namespace

GameCombo::GameCombo(GameList *games, const QString &tool, const QString &settingsKey, QWidget *parent)
	: QComboBox(parent), m_games(games), m_tool(tool), m_settingsKey(settingsKey)
{
	connect(m_games, &GameList::changed, this, &GameCombo::reload);
	reload();
}

const GameCombo::Target *GameCombo::target() const
{
	const int index = currentIndex();
	if (index < 0 || index >= m_targets.size() || launcher::findTool(m_targets[index].app, m_tool).isEmpty())
		return nullptr;
	return &m_targets[index];
}

void GameCombo::save() const
{
	QSettings().setValue(m_settingsKey, currentData());
}

void GameCombo::reload()
{
	const QString previous = count() > 0 ? currentData().toString() : QSettings().value(m_settingsKey).toString();

	m_targets.clear();
	clear();
	// Games without the tool stay listed but disabled, so it is clear why they cannot be used.
	auto *items = qobject_cast<QStandardItemModel *>(model());
	for (const steam::App &app : m_games->apps())
	{
		const bool hasTool = !launcher::findTool(app, m_tool).isEmpty();
		for (const QString &gameDir : app.gameDirs)
		{
			m_targets << Target{app, gameDir};
			QString text = QStringLiteral("%1 — %2").arg(app.name, QFileInfo(gameDir).fileName());
			if (!hasTool)
				text += tr(" (no %1; set a tools folder on the Games tab)").arg(m_tool);
			addItem(text, targetKey(app, gameDir));
			if (!hasTool && items)
				items->item(count() - 1)->setEnabled(false);
		}
	}

	int index = findData(previous);
	if (index < 0 || (items && !items->item(index)->isEnabled()))
	{
		index = -1;
		for (int i = 0; i < count() && index < 0; ++i)
		{
			if (!items || items->item(i)->isEnabled())
				index = i;
		}
	}
	setCurrentIndex(index);
}
