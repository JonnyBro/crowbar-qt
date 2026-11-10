#pragma once

#include <QWidget>

class GameCombo;
class GameList;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;

class ViewTab : public QWidget {
    Q_OBJECT

public:
    explicit ViewTab(GameList *games, QWidget *parent = nullptr);
    ~ViewTab() override;

private:
    void browse();
    void launch(bool withModel);
    void saveSettings() const;

    GameCombo *m_game;
    QLineEdit *m_mdlFile;
    QPushButton *m_view;
    QPushButton *m_openViewer;
    QPlainTextEdit *m_log;
};
