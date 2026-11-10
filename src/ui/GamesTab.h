#pragma once

#include <QWidget>

class GameList;
class QLabel;
class QTreeWidget;

class GamesTab : public QWidget {
    Q_OBJECT

public:
    explicit GamesTab(GameList *games, QWidget *parent = nullptr);

private:
    void reload();

    GameList *m_games;
    QTreeWidget *m_tree;
    QLabel *m_status;
};
