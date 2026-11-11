#include "ui/MainWindow.h"

#include "ui/CompilePage.h"
#include "ui/DownloadPage.h"
#include "ui/GameList.h"
#include "ui/GamesPage.h"
#include "ui/PackPage.h"
#include "ui/PackagesPage.h"
#include "ui/Tasks.h"
#include "ui/TasksPanel.h"
#include "ui/ViewPage.h"
#include "ui/WorkshopPage.h"

#include <QCloseEvent>
#include <QComboBox>
#include <QDragEnterEvent>
#include <QDir>
#include <QDropEvent>
#include <QFileInfo>
#include <QLabel>
#include <QListWidget>
#include <QMimeData>
#include <QSettings>
#include <QSplitter>
#include <QStackedWidget>
#include <QVBoxLayout>

namespace
{

	enum
	{
		PageRole = Qt::UserRole
	};

	QListWidgetItem *addHeading(QListWidget *list, const QString &text)
	{
		auto *item = new QListWidgetItem(text, list);
		item->setFlags(Qt::NoItemFlags);
		QFont font = item->font();
		font.setPointSizeF(font.pointSizeF() * 0.85);
		item->setFont(font);
		return item;
	}

} // namespace

MainWindow::MainWindow(QWidget *parent)
	: QMainWindow(parent), m_games(new GameList(this)), m_tasks(new TaskManager(this)), m_game(new QComboBox(this)),
	  m_gameInfo(new QLabel(this)), m_nav(new QListWidget(this)), m_pages(new QStackedWidget(this)),
	  m_tasksPanel(new TasksPanel(m_tasks, this)), m_vertical(new QSplitter(Qt::Vertical, this))
{
	setWindowTitle(QStringLiteral("Crowbar Qt"));
	setAcceptDrops(true);

	m_compile = new CompilePage(m_games, m_tasks, m_pages);
	m_view = new ViewPage(m_games, m_tasks, m_pages);
	m_packages = new PackagesPage(m_games, m_tasks, m_pages);
	m_pack = new PackPage(m_games, m_tasks, m_pages);

	auto *sidebar = buildSidebar();
	addHeading(m_nav, tr("Models"));
	addPage(Page::Compile, m_compile);
	addPage(Page::View, m_view);
	addHeading(m_nav, tr("Packages"));
	addPage(Page::Packages, m_packages);
	addPage(Page::Pack, m_pack);
	addHeading(m_nav, tr("Workshop"));
	addPage(Page::Workshop, new WorkshopPage(m_games, m_tasks, m_pages));
	addPage(Page::Download, new DownloadPage(m_games, m_tasks, m_pages));
	addHeading(m_nav, QString())->setSizeHint(QSize(0, 8));
	addPage(Page::Games, new GamesPage(m_games, m_tasks, m_pages));

	auto *horizontal = new QSplitter(Qt::Horizontal, this);
	horizontal->addWidget(sidebar);
	horizontal->addWidget(m_pages);
	horizontal->setStretchFactor(1, 1);
	horizontal->setCollapsible(0, false);
	horizontal->setSizes({200, 800});

	m_vertical->addWidget(horizontal);
	m_vertical->addWidget(m_tasksPanel);
	m_vertical->setStretchFactor(0, 3);
	m_vertical->setStretchFactor(1, 1);
	m_vertical->setCollapsible(0, false);
	m_vertical->setCollapsible(1, false);
	setCentralWidget(m_vertical);

	connect(m_nav, &QListWidget::currentRowChanged, this, [this](int row)
			{
		if (row < 0)
			return;
		const QVariant page = m_nav->item(row)->data(PageRole);
		if (page.isValid())
		{
			m_pages->setCurrentIndex(m_pageOrder.indexOf(Page(page.toInt())));
			QSettings().setValue("window/page", page);
		} });
	connect(m_game, &QComboBox::activated, this, [this](int index)
			{
		const QList<GameList::Target> targets = m_games->targets();
		if (index >= 0 && index < targets.size())
			m_games->setCurrent(targets[index].app.id, targets[index].gameDir); });
	connect(m_games, &GameList::changed, this, &MainWindow::reloadGames);
	connect(m_games, &GameList::currentChanged, this, &MainWindow::updateGameInfo);
	// A collapsed panel keeps only its header row; give the space back to the pages.
	connect(m_tasksPanel, &TasksPanel::expandedChanged, this, [this](bool expanded)
			{
		const int total = m_vertical->height();
		const int panel = expanded ? qMax(total / 3, 160) : m_tasksPanel->minimumSizeHint().height();
		m_vertical->setSizes({total - panel, panel}); });

	m_games->refresh();
	showPage(Page(QSettings().value("window/page", int(Page::Compile)).toInt()));

	resize(1100, 720);
	restoreGeometry(QSettings().value("window/geometry").toByteArray());
	m_vertical->restoreState(QSettings().value("window/split").toByteArray());
}

QWidget *MainWindow::buildSidebar()
{
	auto *sidebar = new QWidget(this);
	m_game->setToolTip(tr("The game every page works with"));
	m_game->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon);
	m_game->setMinimumContentsLength(14);
	m_gameInfo->setWordWrap(true);
	m_gameInfo->setForegroundRole(QPalette::PlaceholderText);
	QFont small = m_gameInfo->font();
	small.setPointSizeF(small.pointSizeF() * 0.85);
	m_gameInfo->setFont(small);

	m_nav->setFrameShape(QFrame::NoFrame);
	m_nav->setIconSize(QSize(18, 18));
	m_nav->setSpacing(1);
	m_nav->viewport()->setAutoFillBackground(false);

	auto *layout = new QVBoxLayout(sidebar);
	layout->setContentsMargins(6, 6, 0, 6);
	layout->addWidget(new QLabel(tr("Game"), sidebar));
	layout->addWidget(m_game);
	layout->addWidget(m_gameInfo);
	layout->addSpacing(6);
	layout->addWidget(m_nav, 1);
	return sidebar;
}

void MainWindow::addPage(Page page, QWidget *widget)
{
	static const QHash<int, QPair<QString, QString>> labels = {
		{int(Page::Compile), {"run-build", QT_TR_NOOP("Compile")}},
		{int(Page::View), {"view-preview", QT_TR_NOOP("View")}},
		{int(Page::Packages), {"folder-open", QT_TR_NOOP("Browse Packages")}},
		{int(Page::Pack), {"package-x-generic", QT_TR_NOOP("Pack")}},
		{int(Page::Workshop), {"cloud-upload", QT_TR_NOOP("Your Items")}},
		{int(Page::Download), {"download", QT_TR_NOOP("Download")}},
		{int(Page::Games), {"applications-games", QT_TR_NOOP("Games")}},
	};
	const auto label = labels.value(int(page));
	auto *item = new QListWidgetItem(QIcon::fromTheme(label.first), tr(label.second.toUtf8().constData()), m_nav);
	item->setData(PageRole, int(page));
	m_pageOrder << page;
	m_pages->addWidget(widget);
}

void MainWindow::showPage(Page page)
{
	for (int row = 0; row < m_nav->count(); ++row)
	{
		if (m_nav->item(row)->data(PageRole) == int(page))
		{
			m_nav->setCurrentRow(row);
			return;
		}
	}
	showPage(Page::Compile);
}

void MainWindow::reloadGames()
{
	m_game->clear();
	for (const GameList::Target &target : m_games->targets())
	{
		const QString dir = QFileInfo(target.gameDir).fileName();
		// Most games have one folder; only say which one when there is a choice.
		m_game->addItem(target.app.gameDirs.size() > 1 ? QStringLiteral("%1 — %2").arg(target.app.name, dir) : target.app.name);
	}
	updateGameInfo();
}

void MainWindow::updateGameInfo()
{
	const auto current = m_games->current();
	const QList<GameList::Target> targets = m_games->targets();
	for (int i = 0; i < targets.size(); ++i)
	{
		if (current && targets[i].gameDir == current->gameDir)
			m_game->setCurrentIndex(i);
	}
	if (!current)
	{
		m_gameInfo->setText(m_games->steamFound() ? tr("No Source games found.") : tr("Steam installation not found."));
		return;
	}

	QStringList parts{QFileInfo(current->gameDir).fileName()};
	parts << (current->app.platform == steam::Platform::Windows ? tr("Proton") : tr("native"));
	const QString studiomdl = current->tool("studiomdl");
	if (current->app.toolsPath.isEmpty())
		parts << tr("no tools");
	else if (studiomdl.contains("/win64/") || studiomdl.contains("/x64/") || studiomdl.contains("/linux64/"))
		parts << tr("64-bit tools");
	m_gameInfo->setText(parts.join(QStringLiteral(" · ")));
}

bool MainWindow::openPath(const QString &path)
{
	const QFileInfo info(path);
	if (!info.exists())
		return false;
	const QString suffix = info.suffix().toLower();
	if (info.isDir())
	{
		// A folder with QC files is a model source; anything else is an addon to pack.
		if (!QDir(info.absoluteFilePath()).entryList({"*.qc"}, QDir::Files).isEmpty())
		{
			m_compile->addPath(info.absoluteFilePath());
			showPage(Page::Compile);
		}
		else
		{
			m_pack->setFolder(info.absoluteFilePath());
			showPage(Page::Pack);
		}
		return true;
	}
	if (suffix == "qc")
	{
		m_compile->addPath(info.absoluteFilePath());
		showPage(Page::Compile);
		return true;
	}
	if (suffix == "mdl")
	{
		m_view->setModel(info.absoluteFilePath());
		showPage(Page::View);
		return true;
	}
	if (suffix == "vpk" || suffix == "gma" || suffix == "bin")
	{
		m_packages->openPackage(info.absoluteFilePath());
		showPage(Page::Packages);
		return true;
	}
	return false;
}

void MainWindow::dragEnterEvent(QDragEnterEvent *event)
{
	if (event->mimeData()->hasUrls() && event->mimeData()->urls().first().isLocalFile())
		event->acceptProposedAction();
}

void MainWindow::dropEvent(QDropEvent *event)
{
	bool any = false;
	for (const QUrl &url : event->mimeData()->urls())
	{
		if (url.isLocalFile())
			any = openPath(url.toLocalFile()) || any;
	}
	if (any)
		event->acceptProposedAction();
}

void MainWindow::closeEvent(QCloseEvent *event)
{
	QSettings settings;
	settings.setValue("window/geometry", saveGeometry());
	settings.setValue("window/split", m_vertical->saveState());
	QMainWindow::closeEvent(event);
}
