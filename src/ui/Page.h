#pragma once

#include <QWidget>

class GameList;
class QLabel;
class QVBoxLayout;
class TaskManager;

// Common frame of a sidebar page: a title, then the page's own content.
class Page : public QWidget {
    Q_OBJECT

public:
    Page(const QString &title, GameList *games, TaskManager *tasks, QWidget *parent = nullptr);

protected:
    QVBoxLayout *content() const { return m_content; }
    // Shows a short notice under the title, e.g. why the page cannot run.
    void setNotice(const QString &text);

    GameList *m_games;
    TaskManager *m_tasks;

private:
    QLabel *m_notice;
    QVBoxLayout *m_content;
};
