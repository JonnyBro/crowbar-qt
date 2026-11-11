#pragma once

#include "ui/Page.h"

class QComboBox;
class QLabel;
class QPlainTextEdit;
class QTimer;

class SettingsPage : public Page {
    Q_OBJECT

public:
    SettingsPage(GameList *games, TaskManager *tasks, QWidget *parent = nullptr);

    // Reads the saved settings that affect the core, e.g. at startup.
    static void applySaved();

private:
    void reloadProtons();
    void saveEnvironment();

    QComboBox *m_language;
    QComboBox *m_proton;
    QLabel *m_protonNote;
    QPlainTextEdit *m_environment;
    QLabel *m_environmentNote;
    QTimer *m_environmentTimer;
    QComboBox *m_editor;
};
