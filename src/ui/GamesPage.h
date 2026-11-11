#pragma once

#include "core/Launcher.h"

#include "ui/Page.h"

class QLabel;
class QLineEdit;
class QPushButton;
class QTreeWidget;

class GamesPage : public Page {
    Q_OBJECT

public:
    GamesPage(GameList *games, TaskManager *tasks, QWidget *parent = nullptr);

private:
    void reload();
    // App and game folder of the selected row; false if a folder must be picked first.
    bool selection(steam::App &app, QString &gameDir) const;
    void updateButtons();
    void launch(const launcher::Command &command, const QString &what, bool detached);
    void markCurrent();
    void saveOptions() const;
    void showToolsMenu();
    void addFolder();

    QTreeWidget *m_tree;
    QLabel *m_status;
    QLineEdit *m_options;
    QPushButton *m_runGame;
    QPushButton *m_runHammer;
    QPushButton *m_tools;
    QPushButton *m_useGame;
    QPushButton *m_addFolder;
    quint32 m_optionsAppId = 0;
};
