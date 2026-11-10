#include "ui/GameCombo.h"

#include "core/Launcher.h"
#include "ui/GameList.h"

#include <QFileInfo>
#include <QSettings>

namespace {

QString targetKey(const steam::App &app, const QString &gameDir)
{
    return QStringLiteral("%1:%2").arg(app.id).arg(QFileInfo(gameDir).fileName());
}

} // namespace

GameCombo::GameCombo(GameList *games, const QString &tool, const QString &settingsKey, QWidget *parent)
    : QComboBox(parent)
    , m_games(games)
    , m_tool(tool)
    , m_settingsKey(settingsKey)
{
    connect(m_games, &GameList::changed, this, &GameCombo::reload);
    reload();
}

const GameCombo::Target *GameCombo::target() const
{
    const int index = currentIndex();
    return index >= 0 && index < m_targets.size() ? &m_targets[index] : nullptr;
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
    for (const steam::App &app : m_games->apps()) {
        if (launcher::findTool(app, m_tool).isEmpty())
            continue;
        for (const QString &gameDir : app.gameDirs) {
            m_targets << Target{app, gameDir};
            addItem(QStringLiteral("%1 — %2").arg(app.name, QFileInfo(gameDir).fileName()), targetKey(app, gameDir));
        }
    }

    const int index = findData(previous);
    if (index >= 0)
        setCurrentIndex(index);
}
