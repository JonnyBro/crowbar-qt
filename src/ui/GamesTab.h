#pragma once

#include "core/Launcher.h"

#include <QWidget>

class GameList;
class QLabel;
class QLineEdit;
class QPushButton;
class QTreeWidget;

class GamesTab : public QWidget {
    Q_OBJECT

public:
    explicit GamesTab(GameList *games, QWidget *parent = nullptr);

private:
    void reload();
    // App and game folder of the selected row; false if a folder must be picked first.
    bool selection(steam::App &app, QString &gameDir) const;
    void updateButtons();
    void launch(const launcher::Command &command, const QString &what, bool detached);
    void saveOptions() const;

    GameList *m_games;
    QTreeWidget *m_tree;
    QLabel *m_status;
    QLineEdit *m_options;
    QPushButton *m_runGame;
    QPushButton *m_runHammer;
    quint32 m_optionsAppId = 0;
};
