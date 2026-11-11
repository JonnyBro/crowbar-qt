#include "ui/WorkshopPage.h"

#include "core/Packer.h"
#include "core/Workshop.h"
#include "ui/GameList.h"
#include "ui/PathField.h"
#include "ui/Tasks.h"

#include <QClipboard>
#include <QDesktopServices>
#include <QGuiApplication>
#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QImage>
#include <QLabel>
#include <QLineEdit>
#include <QLocale>
#include <QMenu>
#include <QMessageBox>
#include <QPixmap>
#include <QPushButton>
#include <QRegularExpressionValidator>
#include <QSettings>
#include <QSplitter>
#include <QTreeWidget>
#include <QUrl>
#include <QVBoxLayout>

#include <memory>

namespace
{

	enum Column
	{
		TitleColumn,
		UpdatedColumn,
		SizeColumn,
		IdColumn
	};

	constexpr int IconSize = 48;

} // namespace

WorkshopPage::WorkshopPage(GameList *games, TaskManager *tasks, QWidget *parent)
	: Page(tr("Your Workshop Items"), games, tasks, parent), m_client(new workshop::Client(this)), m_items(new QTreeWidget(this)),
	  m_refresh(new QPushButton(QIcon::fromTheme("view-refresh"), tr("Refresh"), this)), m_itemId(new QLineEdit(this)),
	  m_itemHint(new QLabel(this)), m_content(new QLineEdit(this)), m_icon(new QLineEdit(this)), m_changes(new QLineEdit(this)),
	  m_publish(new QPushButton(this))
{
	setUpPathField(m_content, "publish-content", PathKind::Any);
	setUpPathField(m_icon, "icon", PathKind::File);

	const QSettings settings;
	m_items->setHeaderLabels({tr("Title"), tr("Updated"), tr("Size"), tr("ID")});
	m_items->setRootIsDecorated(false);
	m_items->setIconSize(QSize(IconSize, IconSize));
	m_items->setUniformRowHeights(true);
	m_items->header()->setStretchLastSection(false);
	m_items->header()->setSectionResizeMode(TitleColumn, QHeaderView::Stretch);
	for (int column : {UpdatedColumn, SizeColumn, IdColumn})
		m_items->header()->setSectionResizeMode(column, QHeaderView::ResizeToContents);
	m_items->setContextMenuPolicy(Qt::CustomContextMenu);
	m_items->setToolTip(tr("Select an item to update it. Double-click opens its workshop page."));

	m_content->setText(settings.value("publish/content").toString());
	m_content->setPlaceholderText(tr("A .gma file, or a folder with addon.json to pack first"));
	m_icon->setText(settings.value("publish/icon").toString());
	m_icon->setPlaceholderText(tr("Any image; converted to a 512x512 JPEG if needed"));
	m_changes->setPlaceholderText(tr("Change notes (updates only)"));
	m_itemId->setPlaceholderText(tr("Empty creates a new item"));
	m_itemId->setValidator(new QRegularExpressionValidator(QRegularExpression("\\d{0,20}"), m_itemId));
	m_itemHint->setForegroundRole(QPalette::PlaceholderText);

	auto *newItem = new QPushButton(QIcon::fromTheme("document-new"), tr("New Item"), this);
	auto *idRow = new QHBoxLayout;
	idRow->addWidget(m_itemId, 1);
	idRow->addWidget(m_itemHint, 1);
	idRow->addWidget(newItem);

	auto *contentBrowse = new QPushButton(tr("Browse…"), this);
	auto *contentRow = new QHBoxLayout;
	contentRow->addWidget(m_content, 1);
	contentRow->addWidget(contentBrowse);

	auto *iconBrowse = new QPushButton(tr("Browse…"), this);
	auto *iconRow = new QHBoxLayout;
	iconRow->addWidget(m_icon, 1);
	iconRow->addWidget(iconBrowse);

	auto *topRow = new QHBoxLayout;
	topRow->addStretch(1);
	topRow->addWidget(m_refresh);

	auto *form = new QFormLayout;
	form->addRow(tr("Item ID:"), idRow);
	form->addRow(tr("Content:"), contentRow);
	form->addRow(tr("Icon:"), iconRow);
	form->addRow(tr("Changes:"), m_changes);

	auto *buttons = new QHBoxLayout;
	buttons->addStretch(1);
	buttons->addWidget(m_publish);

	content()->addLayout(topRow);
	content()->addWidget(m_items, 1);
	content()->addLayout(form);
	content()->addLayout(buttons);

	connect(m_refresh, &QPushButton::clicked, this, &WorkshopPage::refreshList);
	connect(newItem, &QPushButton::clicked, this, [this]
			{
		m_items->clearSelection();
		m_itemId->clear(); });
	connect(contentBrowse, &QPushButton::clicked, this, &WorkshopPage::browseContent);
	connect(iconBrowse, &QPushButton::clicked, this, [this]
			{
		const QString file = QFileDialog::getOpenFileName(this, tr("Select icon"), QFileInfo(m_icon->text()).absolutePath(),
														  tr("Images (*.jpg *.jpeg *.png *.bmp *.gif *.webp);;All files (*)"));
		if (!file.isEmpty())
			m_icon->setText(file); });
	connect(m_publish, &QPushButton::clicked, this, &WorkshopPage::publish);
	connect(m_itemId, &QLineEdit::textChanged, this, &WorkshopPage::updateWidgets);
	connect(m_items, &QTreeWidget::itemSelectionChanged, this, [this]
			{
		const QList<QTreeWidgetItem *> selected = m_items->selectedItems();
		if (!selected.isEmpty())
			m_itemId->setText(selected.first()->text(IdColumn)); });
	connect(m_items, &QTreeWidget::itemActivated, this, [](QTreeWidgetItem *item)
			{ QDesktopServices::openUrl(QUrl(publisher::workshopUrl(item->text(IdColumn).toULongLong()))); });
	connect(m_items, &QTreeWidget::customContextMenuRequested, this, [this](const QPoint &pos)
			{
		QTreeWidgetItem *item = m_items->itemAt(pos);
		if (!item)
			return;
		QMenu menu(this);
		menu.addAction(QIcon::fromTheme("internet-web-browser"), tr("Open Workshop Page"), this, [item]
					   { QDesktopServices::openUrl(QUrl(publisher::workshopUrl(item->text(IdColumn).toULongLong()))); });
		menu.addAction(QIcon::fromTheme("edit-copy"), tr("Copy ID"), this, [item]
					   { QGuiApplication::clipboard()->setText(item->text(IdColumn)); });
		menu.exec(m_items->viewport()->mapToGlobal(pos)); });
	connect(m_games, &GameList::currentChanged, this, &WorkshopPage::updateWidgets);

	// Titles, dates and icons come from Steam's public API once gmpublish has listed the IDs.
	connect(m_client, &workshop::Client::detailsListReady, this, [this](const QList<workshop::ItemDetails> &details)
			{
		const QLocale locale;
		for (const workshop::ItemDetails &item : details)
		{
			if (!item.isValid())
				continue;
			for (int i = 0; i < m_items->topLevelItemCount(); ++i)
			{
				QTreeWidgetItem *row = m_items->topLevelItem(i);
				if (row->text(IdColumn).toULongLong() != item.id)
					continue;
				row->setText(UpdatedColumn, locale.toString(item.updated.date(), QLocale::ShortFormat));
				if (!item.previewUrl.isEmpty())
					m_client->fetchData(item.previewUrl, item.id);
			}
		} });
	connect(m_client, &workshop::Client::dataReady, this, [this](quint64 id, const QByteArray &data)
			{
		const QImage image = QImage::fromData(data);
		if (image.isNull())
			return;
		const QPixmap icon = QPixmap::fromImage(image.scaled(IconSize, IconSize, Qt::KeepAspectRatio, Qt::SmoothTransformation));
		for (int i = 0; i < m_items->topLevelItemCount(); ++i)
		{
			if (m_items->topLevelItem(i)->text(IdColumn).toULongLong() == id)
				m_items->topLevelItem(i)->setIcon(TitleColumn, icon);
		} });

	m_itemId->setText(settings.value("publish/id").toString());
	updateWidgets();
}

WorkshopPage::~WorkshopPage()
{
	saveSettings();
}

void WorkshopPage::refreshList()
{
	const auto target = m_games->current();
	if (!target || m_busy)
		return;
	Task *task = m_tasks->start(tr("Your workshop items"));
	auto parser = std::make_shared<publisher::ListParser>();
	connect(task, &Task::lineAdded, this, [parser](const QString &line)
			{ parser->addLine(line); });
	m_busy = true;
	updateWidgets();
	task->run(publisher::listCommand(target->app), [this, task, parser](int exitCode, bool success)
			  {
		m_busy = false;
		updateWidgets();
		if (!success)
		{
			task->appendLine(tr("Could not get the item list (exit code %1). Is Steam running?").arg(exitCode));
			return false;
		}
		showItems(parser->items());
		task->appendLine(tr("%n item(s).", nullptr, int(parser->items().size())));
		return true; });
}

void WorkshopPage::showItems(const QList<publisher::WorkshopItem> &items)
{
	m_items->clear();
	QList<quint64> ids;
	for (const publisher::WorkshopItem &item : items)
	{
		auto *row = new QTreeWidgetItem(m_items);
		row->setText(TitleColumn, item.title);
		row->setText(SizeColumn, item.size);
		row->setText(IdColumn, QString::number(item.id));
		row->setTextAlignment(SizeColumn, Qt::AlignRight | Qt::AlignVCenter);
		row->setIcon(TitleColumn, QIcon::fromTheme("package-x-generic"));
		row->setSizeHint(TitleColumn, QSize(0, IconSize + 6));
		if (QString::number(item.id) == m_itemId->text())
			row->setSelected(true);
		ids << item.id;
	}
	if (!ids.isEmpty())
		m_client->fetchDetailsList(ids);
}

void WorkshopPage::browseContent()
{
	const QString start = QFileInfo(m_content->text()).absolutePath();
	QMenu menu(this);
	const QAction *fileAction = menu.addAction(tr("GMA File…"));
	const QAction *folderAction = menu.addAction(tr("Folder to Pack…"));
	const QAction *chosen = menu.exec(QCursor::pos());
	QString path;
	if (chosen == fileAction)
		path = QFileDialog::getOpenFileName(this, tr("Select GMA file"), start, tr("GMA files (*.gma);;All files (*)"));
	else if (chosen == folderAction)
		path = QFileDialog::getExistingDirectory(this, tr("Select folder with addon.json"), start);
	if (!path.isEmpty())
		m_content->setText(path);
}

void WorkshopPage::publish()
{
	const auto target = m_games->current();
	if (!target || m_busy)
		return;
	saveSettings();

	publisher::Request request;
	request.id = m_itemId->text().toULongLong();
	request.changes = m_changes->text();
	const QString content = m_content->text().trimmed();
	const QString icon = m_icon->text().trimmed();

	const QString what = request.id == 0
							 ? tr("Create a new workshop item from \"%1\"?").arg(QFileInfo(content).fileName())
							 : tr("Upload to workshop item %1?").arg(request.id);
	const QString note = request.id == 0 ? tr("\n\nNew items start hidden; make them public on the workshop page.") : QString();
	if (QMessageBox::question(this, tr("Publish"), what + note) != QMessageBox::Yes)
		return;

	if (!icon.isEmpty())
	{
		QString error;
		request.iconFile = publisher::prepareIcon(icon, m_tempDir.filePath("icon.jpg"), &error);
		if (request.iconFile.isEmpty())
		{
			m_tasks->start(tr("Publish"))->fail(error);
			return;
		}
	}

	if (!QFileInfo(content).isDir())
	{
		request.gmaFile = content;
		upload(request);
		return;
	}

	// A folder is packed with gmad first, into a temporary GMA.
	const QString packed = m_tempDir.filePath(QFileInfo(content).fileName() + ".gma");
	QFile::remove(packed);
	packer::Options options;
	options.warnInvalid = true;
	Task *pack = m_tasks->start(tr("Pack %1").arg(QFileInfo(packed).fileName()));
	m_busy = true;
	updateWidgets();
	pack->run(packer::packCommand(target->app, packer::Format::Gma, content, packed, options), [this, request, packed](int, bool success) mutable
			  {
		m_busy = false;
		const bool ok = success && QFile::exists(packed);
		if (ok)
		{
			request.gmaFile = packed;
			upload(request);
		}
		updateWidgets();
		return ok; });
}

void WorkshopPage::upload(const publisher::Request &request)
{
	const auto target = m_games->current();
	if (!target)
		return;
	Task *task = m_tasks->start(request.id ? tr("Update item %1").arg(request.id) : tr("Publish new item"));
	auto created = std::make_shared<quint64>(0);
	connect(task, &Task::lineAdded, this, [created](const QString &line)
			{
		if (const quint64 id = publisher::parseCreatedId(line))
			*created = id; });
	if (request.iconFile != m_icon->text().trimmed() && !request.iconFile.isEmpty())
		task->appendLine(tr("Icon converted to a 512x512 JPEG."));
	m_busy = true;
	updateWidgets();
	task->run(publisher::publishCommand(target->app, request), [this, task, created](int exitCode, bool success)
			  {
		m_busy = false;
		if (*created)
		{
			m_itemId->setText(QString::number(*created));
			task->appendLine(tr("\nCreated item %1: %2").arg(*created).arg(publisher::workshopUrl(*created)));
		}
		else if (success)
		{
			task->appendLine(tr("\ngmpublish finished; check the log above for errors."));
		}
		else
		{
			task->appendLine(tr("\nPublish failed (exit code %1).").arg(exitCode));
		}
		updateWidgets();
		if (success)
			refreshList();
		return success; });
}

void WorkshopPage::updateWidgets()
{
	const auto target = m_games->current();
	const bool hasTool = target && !target->tool("gmpublish").isEmpty();
	setNotice(!target		 ? tr("No Source game found. Check the Games page.")
			  : !hasTool ? tr("%1 has no gmpublish; workshop publishing works for Garry's Mod.").arg(target->app.name)
						 : QString());
	const bool isNew = m_itemId->text().isEmpty();
	m_itemHint->setText(isNew ? tr("New item") : tr("Update existing item"));
	m_changes->setEnabled(!isNew);
	m_publish->setText(isNew ? tr("Publish New Item") : tr("Update Item"));
	m_publish->setEnabled(hasTool && !m_busy);
	m_refresh->setEnabled(hasTool && !m_busy);
}

void WorkshopPage::saveSettings() const
{
	rememberPath("publish-content", m_content->text());
	rememberPath("icon", m_icon->text());
	QSettings settings;
	settings.setValue("publish/id", m_itemId->text());
	settings.setValue("publish/content", m_content->text());
	settings.setValue("publish/icon", m_icon->text());
}
