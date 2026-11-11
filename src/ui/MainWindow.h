#pragma once

#include <QMainWindow>

class CompilePage;
class GameList;
class PackPage;
class PackagesPage;
class ViewPage;
class QComboBox;
class QLabel;
class QListWidget;
class QSplitter;
class QStackedWidget;
class TaskManager;
class TasksPanel;

class MainWindow : public QMainWindow {
    Q_OBJECT

public:
    explicit MainWindow(QWidget *parent = nullptr);

    // Opens a file or folder on the page that handles it, e.g. a .qc on Compile.
    bool openPath(const QString &path);

protected:
    void closeEvent(QCloseEvent *event) override;
    void dragEnterEvent(QDragEnterEvent *event) override;
    void dropEvent(QDropEvent *event) override;

private:
    enum class Page { Compile, View, Packages, Pack, Workshop, Download, Games, Settings };

    QWidget *buildSidebar();
    void addPage(Page page, QWidget *widget);
    void showPage(Page page);
    void reloadGames();
    void updateGameInfo();

    GameList *m_games;
    TaskManager *m_tasks;
    QComboBox *m_game;
    QLabel *m_gameInfo;
    QListWidget *m_nav;
    QStackedWidget *m_pages;
    TasksPanel *m_tasksPanel;
    QSplitter *m_vertical;
    QList<Page> m_pageOrder;
    CompilePage *m_compile;
    ViewPage *m_view;
    PackagesPage *m_packages;
    PackPage *m_pack;
};
