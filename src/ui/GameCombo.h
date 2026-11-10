#pragma once

#include "core/Steam.h"

#include <QComboBox>

class GameList;

// Lists every game folder of the games that ship the given tool.
class GameCombo : public QComboBox {
    Q_OBJECT

public:
    struct Target {
        steam::App app;
        QString gameDir;
    };

    GameCombo(GameList *games, const QString &tool, const QString &settingsKey, QWidget *parent = nullptr);

    const Target *target() const;
    void save() const;

private:
    void reload();

    GameList *m_games;
    QString m_tool;
    QString m_settingsKey;
    QList<Target> m_targets;
};
