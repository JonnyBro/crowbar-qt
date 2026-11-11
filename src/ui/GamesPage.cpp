#include "ui/GamesPage.h"

#include "core/Runner.h"
#include "ui/GameList.h"
#include "ui/Tasks.h"

#include <QDesktopServices>
#include <QDir>
#include <QFileDialog>
#include <QMenu>
#include <QFileInfo>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QProcess>
#include <QSettings>
#include <QPushButton>
#include <QTreeWidget>
#include <QUrl>
#include <QVBoxLayout>

namespace
{

	enum Column
	{
		Name,
		AppId,
		Build,
		CompatTool,
		Tools,
		Location,
		ColumnCount
	};

	enum
	{
		AppIndexRole = Qt::UserRole
	};

	QString optionsKey(quint32 appId)
	{
		return QStringLiteral("games/%1/options").arg(appId);
	}

}

GamesPage::GamesPage(GameList *games, TaskManager *tasks, QWidget *parent)
	: Page(tr("Games"), games, tasks, parent), m_tree(new QTreeWidget(this)), m_status(new QLabel(this)), m_options(new QLineEdit(this)), m_runGame(new QPushButton(tr("Run Game"), this)), m_runHammer(new QPushButton(tr("Run Hammer"), this)), m_tools(new QPushButton(tr("Tools Folder…"), this)), m_useGame(new QPushButton(tr("Work With This Game"), this)), m_addFolder(new QPushButton(QIcon::fromTheme("list-add"), tr("Add Game Folder…"), this))
{
	m_options->setPlaceholderText(tr("Game launch options, e.g. -dev -console +map gm_construct"));
	m_tree->setColumnCount(ColumnCount);
	m_tree->setHeaderLabels({tr("Game"), tr("App ID"), tr("Build"), tr("Proton"), tr("Tools"), tr("Location")});
	m_tree->setRootIsDecorated(true);
	m_tree->setAlternatingRowColors(true);
	m_tree->setUniformRowHeights(true);
	m_tree->header()->setStretchLastSection(true);

	auto *refreshButton = new QPushButton(tr("Refresh"), this);

	auto *bar = new QHBoxLayout;
	bar->addWidget(m_status, 1);
	bar->addWidget(m_addFolder);
	bar->addWidget(m_useGame);
	bar->addWidget(m_tools);
	bar->addWidget(refreshButton);

	auto *runBar = new QHBoxLayout;
	runBar->addWidget(new QLabel(tr("Launch options:"), this));
	runBar->addWidget(m_options, 1);
	runBar->addWidget(m_runGame);
	runBar->addWidget(m_runHammer);

	content()->addLayout(bar);
	content()->addWidget(m_tree, 1);
	content()->addLayout(runBar);

	m_tools->setToolTip(tr("Use tools (studiomdl, hlmv, …) from another folder, e.g. the bin folder of the game's Windows build."));
	connect(m_tools, &QPushButton::clicked, this, &GamesPage::showToolsMenu);
	connect(m_tree, &QTreeWidget::itemSelectionChanged, this, &GamesPage::updateButtons);
	connect(m_options, &QLineEdit::editingFinished, this, &GamesPage::saveOptions);
	connect(m_runGame, &QPushButton::clicked, this, [this]
			{
        steam::App app;
        QString gameDir;
        if (!selection(app, gameDir))
            return;
        saveOptions();
        // Detached: if Steam is not running yet, this process becomes the Steam client
        // and must survive this app closing.
        launch(runner::gameCommand(app, gameDir, QProcess::splitCommand(m_options->text())), app.name, true); });
	connect(m_runHammer, &QPushButton::clicked, this, [this]
			{
        steam::App app;
        QString gameDir;
        if (selection(app, gameDir))
            launch(runner::hammerCommand(app, gameDir), tr("Hammer"), false); });

	connect(refreshButton, &QPushButton::clicked, m_games, &GameList::refresh);
	connect(m_games, &GameList::changed, this, &GamesPage::reload);
	connect(m_tree, &QTreeWidget::itemActivated, this, [this]
			{
		steam::App app;
		QString gameDir;
		if (selection(app, gameDir))
			m_games->setCurrent(gameDir); });
	connect(m_useGame, &QPushButton::clicked, this, [this]
			{
		steam::App app;
		QString gameDir;
		if (selection(app, gameDir))
			m_games->setCurrent(gameDir); });
	connect(m_games, &GameList::currentChanged, this, &GamesPage::markCurrent);
	m_addFolder->setToolTip(tr("Add a game or mod folder with a gameinfo.txt that Steam does not list, e.g. a mod kept elsewhere."));
	connect(m_addFolder, &QPushButton::clicked, this, &GamesPage::addFolder);
	m_tree->setContextMenuPolicy(Qt::CustomContextMenu);
	connect(m_tree, &QTreeWidget::customContextMenuRequested, this, [this](const QPoint &pos)
			{
		QTreeWidgetItem *item = m_tree->itemAt(pos);
		if (!item)
			return;
		QMenu menu(this);
		menu.addAction(QIcon::fromTheme("folder-open"), tr("Open Folder"), this, [item]
					   { QDesktopServices::openUrl(QUrl::fromLocalFile(item->text(Location))); });
		const QString location = QDir::cleanPath(item->text(Location));
		if (GameList::customFolders().contains(location))
			menu.addAction(QIcon::fromTheme("list-remove"), tr("Remove From List"), this, [this, location]
						   {
				GameList::removeCustomFolder(location);
				m_games->refresh(); });
		menu.exec(m_tree->viewport()->mapToGlobal(pos)); });

	reload();
}

void GamesPage::reload()
{
	m_tree->clear();

	for (const steam::App &app : m_games->apps())
	{
		const bool windows = app.platform == steam::Platform::Windows;
		auto *item = new QTreeWidgetItem(m_tree);
		const int appIndex = int(&app - m_games->apps().data());
		item->setData(Name, AppIndexRole, appIndex);
		item->setText(Name, app.isMod ? tr("%1 (mod)").arg(app.name) : app.name);
		if (app.isMod)
			item->setToolTip(Name, app.baseName.isEmpty() ? tr("Runs on app %1, which is not installed.").arg(app.id)
														  : tr("Runs on %1.").arg(app.baseName));
		item->setText(AppId, QString::number(app.id));
		item->setText(Build, windows ? tr("Windows") : tr("Linux"));
		if (windows)
			item->setText(CompatTool, app.protonPath.isEmpty() ? tr("not found") : app.compatToolName);
		item->setText(Tools, app.toolsPath.isEmpty() ? tr("missing") : app.toolsOverridden ? tr("custom")
																						   : tr("found"));
		item->setToolTip(Tools, app.toolsPath.isEmpty()
									? tr("No modding tools in this game's bin folder. Use \"Tools Folder…\" to point at "
										 "the bin folder of the Windows build or of another game.")
									: app.toolsPath);
		if (!windows && !app.compatToolName.isEmpty())
			item->setText(CompatTool, tr("%1 (tools)").arg(app.compatToolName));
		item->setToolTip(CompatTool, app.protonPath);
		item->setText(Location, app.installPath);

		for (const QString &gameDir : app.gameDirs)
		{
			auto *child = new QTreeWidgetItem(item);
			child->setData(Name, AppIndexRole, appIndex);
			child->setText(Name, QFileInfo(gameDir).fileName());
			child->setText(Location, gameDir);
		}
		if (app.toolsPath.isEmpty())
		{
			const QBrush grey = palette().brush(QPalette::Disabled, QPalette::Text);
			QList<QTreeWidgetItem *> rows{item};
			for (int i = 0; i < item->childCount(); ++i)
				rows << item->child(i);
			for (QTreeWidgetItem *row : rows)
			{
				for (int column = 0; column < ColumnCount; ++column)
					row->setForeground(column, grey);
			}
		}
	}

	for (int column = 0; column < Location; ++column)
		m_tree->resizeColumnToContents(column);

	if (!m_games->steamFound())
		m_status->setText(tr("Steam installation not found."));
	else
		m_status->setText(tr("%n Source game(s) found.", nullptr, int(m_games->apps().size())));
	m_tree->expandAll();
	markCurrent();
	updateButtons();
}

bool GamesPage::selection(steam::App &app, QString &gameDir) const
{
	const QList<QTreeWidgetItem *> selected = m_tree->selectedItems();
	if (selected.isEmpty())
		return false;
	const QTreeWidgetItem *item = selected.first();
	const int index = item->data(Name, AppIndexRole).toInt();
	if (index < 0 || index >= m_games->apps().size())
		return false;
	app = m_games->apps().at(index);
	if (item->parent())
		gameDir = item->text(Location);
	else if (app.gameDirs.size() == 1)
		gameDir = app.gameDirs.first();
	return !gameDir.isEmpty();
}

void GamesPage::updateButtons()
{
	steam::App app;
	QString gameDir;
	const bool ready = selection(app, gameDir);
	m_runGame->setEnabled(ready && app.id != 0);
	m_useGame->setEnabled(ready);
	m_tools->setEnabled(app.id != 0);
	m_runHammer->setEnabled(ready && !launcher::findTool(app, "hammer").isEmpty());
	m_runGame->setToolTip(ready || !app.id ? QString() : tr("Select one of the game's folders."));
	saveOptions();
	m_optionsAppId = app.id;
	m_options->setEnabled(app.id != 0);
	m_options->setText(app.id ? QSettings().value(optionsKey(app.id)).toString() : QString());
}

void GamesPage::saveOptions() const
{
	if (m_optionsAppId)
		QSettings().setValue(optionsKey(m_optionsAppId), m_options->text());
}

void GamesPage::launch(const launcher::Command &command, const QString &what, bool detached)
{
	Task *task = m_tasks->start(tr("Run %1").arg(what));
	if (!command.isValid())
	{
		task->fail(command.error);
		return;
	}
	if (detached)
	{
		task->appendLine(command.displayString());
		if (!QProcess::startDetached(command.program, command.arguments, command.workingDirectory))
		{
			task->fail(tr("Could not start \"%1\".").arg(command.program));
			return;
		}
		task->appendLine(tr("Asked Steam to start %1.").arg(what));
		task->finish(Task::State::Succeeded);
		return;
	}
	// Closing the program is the normal way for this task to end.
	task->run(command, [](int, bool)
			  { return true; });
}

void GamesPage::addFolder()
{
	const QString dir = QFileDialog::getExistingDirectory(this, tr("Select a game folder (with gameinfo.txt)"));
	if (dir.isEmpty())
		return;
	if (!steam::appForGameDir(dir, m_games->apps()))
	{
		QMessageBox::warning(this, tr("Add Game Folder"), tr("\"%1\" has no gameinfo.txt. Pick the game folder itself, e.g. \"hl2\" or a mod folder.").arg(dir));
		return;
	}
	GameList::addCustomFolder(dir);
	m_games->refresh();
	for (const GameList::Target &target : m_games->targets())
	{
		if (target.gameDir == QDir::cleanPath(dir))
			m_games->setCurrent(target.gameDir);
	}
}

void GamesPage::markCurrent()
{
	const auto current = m_games->current();
	for (int i = 0; i < m_tree->topLevelItemCount(); ++i)
	{
		QTreeWidgetItem *item = m_tree->topLevelItem(i);
		for (int j = 0; j < item->childCount(); ++j)
		{
			QTreeWidgetItem *child = item->child(j);
			const bool isCurrent = current && child->text(Location) == current->gameDir;
			QFont font = child->font(Name);
			font.setBold(isCurrent);
			child->setFont(Name, font);
			child->setIcon(Name, isCurrent ? QIcon::fromTheme("checkmark") : QIcon());
		}
	}
}

void GamesPage::showToolsMenu()
{
	steam::App app;
	QString gameDir;
	selection(app, gameDir);
	if (!app.id)
		return;

	QMenu menu(this);
	QAction *choose = menu.addAction(tr("Choose Folder…"));
	QMenu *from = menu.addMenu(tr("Use Tools From"));
	for (const steam::App &other : m_games->apps())
	{
		if (other.id == app.id || other.toolsPath.isEmpty() || other.toolsOverridden)
			continue;
		const QString path = other.toolsPath;
		from->addAction(other.name, this, [this, id = app.id, path]
						{
            GameList::setToolsOverride(id, path);
            m_games->refresh(); });
	}
	from->setEnabled(!from->isEmpty());
	QAction *reset = menu.addAction(tr("Automatic (game's own bin folder)"));
	reset->setEnabled(app.toolsOverridden);

	const QAction *chosen = menu.exec(m_tools->mapToGlobal(QPoint(0, m_tools->height())));
	if (chosen == choose)
	{
		const QString start = app.toolsPath.isEmpty() ? app.installPath : app.toolsPath;
		const QString dir = QFileDialog::getExistingDirectory(this, tr("Select tools folder for %1").arg(app.name), start);
		if (dir.isEmpty())
			return;
		GameList::setToolsOverride(app.id, dir);
		m_games->refresh();
	}
	else if (chosen == reset)
	{
		GameList::setToolsOverride(app.id, {});
		m_games->refresh();
	}
}
