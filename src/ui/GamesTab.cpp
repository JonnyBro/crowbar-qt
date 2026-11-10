#include "ui/GamesTab.h"

#include "core/Runner.h"
#include "core/ToolProcess.h"
#include "ui/GameList.h"

#include <QDesktopServices>
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

GamesTab::GamesTab(GameList *games, QWidget *parent)
	: QWidget(parent), m_games(games), m_tree(new QTreeWidget(this)), m_status(new QLabel(this)), m_options(new QLineEdit(this)), m_runGame(new QPushButton(tr("Run Game"), this)), m_runHammer(new QPushButton(tr("Run Hammer"), this)), m_tools(new QPushButton(tr("Tools Folder…"), this))
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
	bar->addWidget(m_tools);
	bar->addWidget(refreshButton);

	auto *runBar = new QHBoxLayout;
	runBar->addWidget(new QLabel(tr("Launch options:"), this));
	runBar->addWidget(m_options, 1);
	runBar->addWidget(m_runGame);
	runBar->addWidget(m_runHammer);

	auto *layout = new QVBoxLayout(this);
	layout->addLayout(bar);
	layout->addWidget(m_tree);
	layout->addLayout(runBar);

	m_tools->setToolTip(tr("Use tools (studiomdl, hlmv, …) from another folder, e.g. the bin folder of the game's Windows build."));
	connect(m_tools, &QPushButton::clicked, this, &GamesTab::showToolsMenu);
	connect(m_tree, &QTreeWidget::itemSelectionChanged, this, &GamesTab::updateButtons);
	connect(m_options, &QLineEdit::editingFinished, this, &GamesTab::saveOptions);
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
	connect(m_games, &GameList::changed, this, &GamesTab::reload);
	connect(m_tree, &QTreeWidget::itemActivated, this, [](QTreeWidgetItem *item)
			{ QDesktopServices::openUrl(QUrl::fromLocalFile(item->text(Location))); });

	reload();
}

void GamesTab::reload()
{
	m_tree->clear();

	for (const steam::App &app : m_games->apps())
	{
		const bool windows = app.platform == steam::Platform::Windows;
		auto *item = new QTreeWidgetItem(m_tree);
		const int appIndex = int(&app - m_games->apps().data());
		item->setData(Name, AppIndexRole, appIndex);
		item->setText(Name, app.name);
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
	updateButtons();
}

bool GamesTab::selection(steam::App &app, QString &gameDir) const
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

void GamesTab::updateButtons()
{
	steam::App app;
	QString gameDir;
	const bool ready = selection(app, gameDir);
	m_runGame->setEnabled(ready);
	m_tools->setEnabled(app.id != 0);
	m_runHammer->setEnabled(ready && !launcher::findTool(app, "hammer").isEmpty());
	m_runGame->setToolTip(ready || !app.id ? QString() : tr("Select one of the game's folders."));
	saveOptions();
	m_optionsAppId = app.id;
	m_options->setEnabled(app.id != 0);
	m_options->setText(app.id ? QSettings().value(optionsKey(app.id)).toString() : QString());
}

void GamesTab::saveOptions() const
{
	if (m_optionsAppId)
		QSettings().setValue(optionsKey(m_optionsAppId), m_options->text());
}

void GamesTab::launch(const launcher::Command &command, const QString &what, bool detached)
{
	if (!command.isValid())
	{
		QMessageBox::warning(this, tr("Run"), command.error);
		return;
	}
	if (detached)
	{
		if (!QProcess::startDetached(command.program, command.arguments, command.workingDirectory))
			QMessageBox::warning(this, tr("Run"), tr("Could not start \"%1\".").arg(command.program));
		else
			m_status->setText(tr("Asked Steam to start %1.").arg(what));
		return;
	}
	auto *process = new ToolProcess(this);
	connect(process, &ToolProcess::failedToStart, this, [this, process](const QString &error)
			{
        QMessageBox::warning(this, tr("Run"), error);
        process->deleteLater(); });
	connect(process, &ToolProcess::finished, process, &QObject::deleteLater);
	m_status->setText(tr("Starting %1…").arg(what));
	process->start(command);
}

void GamesTab::showToolsMenu()
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
