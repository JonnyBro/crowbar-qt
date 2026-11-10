#include "ui/GamesTab.h"

#include "ui/GameList.h"

#include <QDesktopServices>
#include <QFileInfo>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QPushButton>
#include <QTreeWidget>
#include <QUrl>
#include <QVBoxLayout>

namespace {

enum Column { Name, AppId, Build, CompatTool, Tools, Location, ColumnCount };

}

GamesTab::GamesTab(GameList *games, QWidget *parent)
    : QWidget(parent)
    , m_games(games)
    , m_tree(new QTreeWidget(this))
    , m_status(new QLabel(this))
{
    m_tree->setColumnCount(ColumnCount);
    m_tree->setHeaderLabels({tr("Game"), tr("App ID"), tr("Build"), tr("Proton"), tr("Tools"), tr("Location")});
    m_tree->setRootIsDecorated(true);
    m_tree->setAlternatingRowColors(true);
    m_tree->setUniformRowHeights(true);
    m_tree->header()->setStretchLastSection(true);

    auto *refreshButton = new QPushButton(tr("Refresh"), this);

    auto *bar = new QHBoxLayout;
    bar->addWidget(m_status, 1);
    bar->addWidget(refreshButton);

    auto *layout = new QVBoxLayout(this);
    layout->addLayout(bar);
    layout->addWidget(m_tree);

    connect(refreshButton, &QPushButton::clicked, m_games, &GameList::refresh);
    connect(m_games, &GameList::changed, this, &GamesTab::reload);
    connect(m_tree, &QTreeWidget::itemActivated, this, [](QTreeWidgetItem *item) {
        QDesktopServices::openUrl(QUrl::fromLocalFile(item->text(Location)));
    });

    reload();
}

void GamesTab::reload()
{
    m_tree->clear();

    for (const steam::App &app : m_games->apps()) {
        const bool windows = app.platform == steam::Platform::Windows;
        auto *item = new QTreeWidgetItem(m_tree);
        item->setText(Name, app.name);
        item->setText(AppId, QString::number(app.id));
        item->setText(Build, windows ? tr("Windows") : tr("Linux"));
        if (windows)
            item->setText(CompatTool, app.protonPath.isEmpty() ? tr("not found") : app.compatToolName);
        item->setText(Tools, app.toolsPath.isEmpty() ? tr("missing") : tr("found"));
        item->setToolTip(Tools, app.toolsPath);
        item->setToolTip(CompatTool, app.protonPath);
        item->setText(Location, app.installPath);

        for (const QString &gameDir : app.gameDirs) {
            auto *child = new QTreeWidgetItem(item);
            child->setText(Name, QFileInfo(gameDir).fileName());
            child->setText(Location, gameDir);
        }
    }

    for (int column = 0; column < Location; ++column)
        m_tree->resizeColumnToContents(column);

    if (!m_games->steamFound())
        m_status->setText(tr("Steam installation not found."));
    else
        m_status->setText(tr("%n Source game(s) found.", nullptr, int(m_games->apps().size())));
}
