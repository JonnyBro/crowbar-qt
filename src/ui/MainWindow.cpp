#include "ui/MainWindow.h"

#include "ui/CompileTab.h"
#include "ui/DownloadTab.h"
#include "ui/GameList.h"
#include "ui/GamesTab.h"
#include "ui/PackTab.h"
#include "ui/PublishTab.h"
#include "ui/UnpackTab.h"
#include "ui/ViewTab.h"

#include <QCloseEvent>
#include <QSettings>
#include <QTabWidget>

MainWindow::MainWindow(QWidget *parent)
    : QMainWindow(parent)
{
    setWindowTitle(QStringLiteral("Crowbar Qt"));

    auto *games = new GameList(this);
    games->refresh();

    auto *tabs = new QTabWidget(this);
    tabs->addTab(new GamesTab(games, tabs), tr("Games"));
    tabs->addTab(new CompileTab(games, tabs), tr("Compile"));
    tabs->addTab(new ViewTab(games, tabs), tr("View"));
    tabs->addTab(new PackTab(games, tabs), tr("Pack"));
    tabs->addTab(new UnpackTab(tabs), tr("Unpack"));
    tabs->addTab(new PublishTab(games, tabs), tr("Publish"));
    tabs->addTab(new DownloadTab(games, tabs), tr("Download"));
    setCentralWidget(tabs);

    resize(1000, 600);
    restoreGeometry(QSettings().value("window/geometry").toByteArray());
}

void MainWindow::closeEvent(QCloseEvent *event)
{
    QSettings().setValue("window/geometry", saveGeometry());
    QMainWindow::closeEvent(event);
}
