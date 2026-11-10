#include "ui/MainWindow.h"

#include "ui/CompileTab.h"
#include "ui/GameList.h"
#include "ui/GamesTab.h"

#include <QCloseEvent>
#include <QSettings>
#include <QTabWidget>

MainWindow::MainWindow(QWidget *parent)
    : QMainWindow(parent)
{
    setWindowTitle(tr("Crowbar"));

    auto *games = new GameList(this);
    games->refresh();

    auto *tabs = new QTabWidget(this);
    tabs->addTab(new GamesTab(games, tabs), tr("Games"));
    tabs->addTab(new CompileTab(games, tabs), tr("Compile"));
    setCentralWidget(tabs);

    resize(1000, 600);
    restoreGeometry(QSettings().value("window/geometry").toByteArray());
}

void MainWindow::closeEvent(QCloseEvent *event)
{
    QSettings().setValue("window/geometry", saveGeometry());
    QMainWindow::closeEvent(event);
}
