#pragma once

#include "core/Steam.h"

#include <QWidget>

class GameList;
class QCheckBox;
class QComboBox;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class ToolProcess;

class CompileTab : public QWidget {
    Q_OBJECT

public:
    explicit CompileTab(GameList *games, QWidget *parent = nullptr);
    ~CompileTab() override;

private:
    struct Target {
        steam::App app;
        QString gameDir;
    };

    void reloadGames();
    void browse();
    void compile();
    void setRunning(bool running);
    void saveSettings() const;

    GameList *m_games;
    ToolProcess *m_process;
    QList<Target> m_targets;

    QComboBox *m_game;
    QLineEdit *m_qcFile;
    QCheckBox *m_noP4;
    QCheckBox *m_verbose;
    QLineEdit *m_extraOptions;
    QPushButton *m_compile;
    QPushButton *m_cancel;
    QPlainTextEdit *m_log;
};
