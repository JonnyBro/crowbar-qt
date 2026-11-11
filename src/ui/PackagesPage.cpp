#include "ui/PackagesPage.h"

#include "ui/GameList.h"
#include "ui/PathField.h"
#include "ui/Tasks.h"
#include "ui/TempDir.h"

#include <QComboBox>
#include <QDesktopServices>
#include <QDirIterator>
#include <QFileDialog>
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHash>
#include <QHeaderView>
#include <QImage>
#include <QImageReader>
#include <QLabel>
#include <QLineEdit>
#include <QLocale>
#include <QMimeData>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QApplication>
#include <QCoreApplication>
#include <QPointer>
#include <QRegularExpression>
#include <QScrollArea>
#include <QSet>
#include <QSettings>
#include <QSplitter>
#include <QStackedWidget>
#include <QThread>
#include <QTimer>
#include <QTreeWidget>
#include <QUrl>
#include <QVBoxLayout>

#include <algorithm>
#include <atomic>
#include <memory>
#include <functional>

namespace
{

	enum OutputMode
	{
		PackageSubfolder,
		WorkFolder
	};

	enum
	{
		EntryIndexRole = Qt::UserRole,
		PathRole
	};

	constexpr quint64 MaxPreviewSize = 4 * 1024 * 1024;
	constexpr int MaxSearchResults = 5000;

	enum Scope
	{
		ThisPackage,
		AllPackages
	};

	enum
	{
		ResultFileRole = Qt::UserRole + 10,
		ResultEntryRole
	};

	// Packages a game loads: its VPKs (one per multi-part set) and its addons.
	QStringList gamePackages(const GameList::Target &target, const QStringList &libraries)
	{
		static const QRegularExpression chunk("_\\d{3}\\.vpk$", QRegularExpression::CaseInsensitiveOption);
		QStringList packages;
		QDirIterator vpks(target.app.installPath, {"*.vpk"}, QDir::Files, QDirIterator::Subdirectories);
		while (vpks.hasNext())
		{
			const QString file = vpks.next();
			if (!chunk.match(file).hasMatch())
				packages << file;
		}
		QDirIterator addons(target.gameDir + "/addons", {"*.gma"}, QDir::Files);
		while (addons.hasNext())
			packages << addons.next();
		for (const QString &library : libraries)
		{
			QDirIterator workshop(QStringLiteral("%1/steamapps/workshop/content/%2").arg(library).arg(target.app.id), {"*.gma"},
								  QDir::Files, QDirIterator::Subdirectories);
			while (workshop.hasNext())
				packages << workshop.next();
		}
		packages.removeDuplicates();
		packages.sort();
		return packages;
	}

	void collectEntries(const QTreeWidgetItem *item, QSet<int> &indexes)
	{
		const int index = item->data(0, EntryIndexRole).toInt();
		if (index >= 0)
			indexes.insert(index);
		for (int i = 0; i < item->childCount(); ++i)
			collectEntries(item->child(i), indexes);
	}

	bool isTextEntry(const QString &path)
	{
		static const QStringList text{"txt", "lua", "vmt", "qc", "qci", "smd", "json", "cfg", "res", "vdf", "kv", "ini", "md", "fgd", "scr", "rad", "nut", "properties"};
		return text.contains(QFileInfo(path).suffix().toLower());
	}

	// Lets files be dragged out of the package into a file manager.
	class PackageTree : public QTreeWidget
	{
	public:
		using Extractor = std::function<QStringList(const QList<int> &)>;

		PackageTree(Extractor extractor, QWidget *parent)
			: QTreeWidget(parent), m_extractor(std::move(extractor))
		{
			setDragEnabled(true);
			setDragDropMode(QAbstractItemView::DragOnly);
		}

	protected:
		QStringList mimeTypes() const override
		{
			return {"text/uri-list"};
		}

		QMimeData *mimeData(const QList<QTreeWidgetItem *> &items) const override
		{
			QSet<int> indexes;
			for (const QTreeWidgetItem *item : items)
				collectEntries(item, indexes);
			QList<int> sorted(indexes.begin(), indexes.end());
			std::sort(sorted.begin(), sorted.end());
			QList<QUrl> urls;
			// Whole folders are dragged as their files; the paths stay relative to the package.
			for (const QString &file : m_extractor(sorted))
				urls << QUrl::fromLocalFile(file);
			auto *mime = new QMimeData;
			mime->setUrls(urls);
			return mime;
		}

	private:
		Extractor m_extractor;
	};

} // namespace

PackagesPage::PackagesPage(GameList *games, TaskManager *tasks, QWidget *parent)
	: Page(tr("Packages"), games, tasks, parent), m_file(new QLineEdit(this)), m_info(new QLabel(this)), m_filter(new QLineEdit(this)),
	  m_filterTimer(new QTimer(this)), m_scope(new QComboBox(this)), m_search(new QPushButton(QIcon::fromTheme("edit-find"), tr("Search"), this)),
	  m_left(new QStackedWidget(this)), m_results(new QTreeWidget(this)), m_tree(new PackageTree([this](const QList<int> &indexes)
															  { return extractToTemp(indexes); }, this)),
	  m_preview(new QStackedWidget(this)), m_previewMessage(new QLabel(this)), m_previewText(new QPlainTextEdit(this)),
	  m_previewImageArea(new QScrollArea(this)), m_previewImage(new QLabel(this)), m_outputMode(new QComboBox(this)),
	  m_outputPath(new QLineEdit(this)), m_outputBrowse(new QPushButton(tr("Browse…"), this)),
	  m_unpackAll(new QPushButton(tr("Unpack All"), this)), m_unpackSelected(new QPushButton(tr("Unpack Selected"), this))
{
	setUpPathField(m_file, "package", PathKind::File);
	setUpPathField(m_outputPath, "unpack-output", PathKind::Folder);

	const QSettings settings;
	m_file->setText(settings.value("unpack/file").toString());
	m_file->setPlaceholderText(tr("Path to a .vpk, .gma or workshop .bin file"));
	m_filter->setPlaceholderText(tr("Search files"));
	m_filter->setClearButtonEnabled(true);
	m_filterTimer->setSingleShot(true);
	m_filterTimer->setInterval(200);

	m_tree->setHeaderLabels({tr("Name"), tr("Size")});
	m_tree->setSelectionMode(QAbstractItemView::ExtendedSelection);
	m_tree->setUniformRowHeights(true);
	m_tree->header()->setStretchLastSection(false);
	m_tree->header()->setSectionResizeMode(0, QHeaderView::Stretch);
	// Fixed width: sizing to contents only measures rows that are expanded at that moment.
	m_tree->header()->setSectionResizeMode(1, QHeaderView::Fixed);
	m_tree->header()->resizeSection(1, m_tree->fontMetrics().horizontalAdvance("0000,00 MiB") + 32);
	m_tree->setToolTip(tr("Double-click opens a file; drag files out to copy them."));

	m_previewMessage->setAlignment(Qt::AlignCenter);
	m_previewMessage->setWordWrap(true);
	m_previewMessage->setForegroundRole(QPalette::PlaceholderText);
	m_previewText->setReadOnly(true);
	m_previewText->setLineWrapMode(QPlainTextEdit::NoWrap);
	m_previewText->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
	m_previewImage->setAlignment(Qt::AlignCenter);
	m_previewImageArea->setWidget(m_previewImage);
	m_previewImageArea->setWidgetResizable(true);
	m_preview->addWidget(m_previewMessage);
	m_preview->addWidget(m_previewText);
	m_preview->addWidget(m_previewImageArea);

	m_outputMode->addItem(tr("Subfolder (of package)"), PackageSubfolder);
	m_outputMode->addItem(tr("Work folder"), WorkFolder);
	m_outputMode->setCurrentIndex(qBound(0, settings.value("unpack/outputMode", 0).toInt(), 1));
	m_outputPath->setText(settings.value("unpack/outputPath").toString());
	m_outputPath->setPlaceholderText(tr("Folder for unpacked files"));

	auto *browse = new QPushButton(tr("Browse…"), this);
	auto *fileRow = new QHBoxLayout;
	fileRow->addWidget(m_file, 1);
	fileRow->addWidget(browse);

	m_scope->addItem(tr("In this package"), ThisPackage);
	m_scope->addItem(tr("In all of the game's packages"), AllPackages);
	m_results->setHeaderLabels({tr("File"), tr("Size")});
	m_results->setUniformRowHeights(true);
	m_results->header()->setStretchLastSection(false);
	m_results->header()->setSectionResizeMode(0, QHeaderView::Stretch);
	m_results->header()->setSectionResizeMode(1, QHeaderView::Fixed);
	m_results->header()->resizeSection(1, m_tree->header()->sectionSize(1));
	m_results->setToolTip(tr("Double-click a file to open its package on it."));
	m_left->addWidget(m_tree);
	m_left->addWidget(m_results);

	auto *filterRow = new QHBoxLayout;
	filterRow->addWidget(m_scope);
	filterRow->addWidget(m_filter, 1);
	filterRow->addWidget(m_search);
	filterRow->addWidget(m_info);

	auto *split = new QSplitter(Qt::Horizontal, this);
	split->addWidget(m_left);
	split->addWidget(m_preview);
	split->setStretchFactor(0, 3);
	split->setStretchFactor(1, 2);

	auto *outputRow = new QHBoxLayout;
	outputRow->addWidget(m_outputMode);
	outputRow->addWidget(m_outputPath, 3);
	outputRow->addWidget(m_outputBrowse);
	outputRow->addStretch(1);
	outputRow->addWidget(m_unpackSelected);
	outputRow->addWidget(m_unpackAll);

	auto *form = new QFormLayout;
	form->addRow(tr("Package:"), fileRow);

	content()->addLayout(form);
	content()->addLayout(filterRow);
	content()->addWidget(split, 1);
	content()->addLayout(outputRow);

	connect(browse, &QPushButton::clicked, this, [this]
			{
		const QString file = QFileDialog::getOpenFileName(this, tr("Select package"), QFileInfo(m_file->text()).absolutePath(),
														  tr("Packages (*.vpk *.gma *.bin);;All files (*)"));
		if (!file.isEmpty())
			openPackage(file); });
	connect(m_file, &QLineEdit::editingFinished, this, &PackagesPage::loadPackage);
	connect(m_filter, &QLineEdit::textChanged, this, [this]
			{
		if (m_scope->currentData().toInt() == ThisPackage)
			m_filterTimer->start(); });
	connect(m_filter, &QLineEdit::returnPressed, this, [this]
			{
		if (m_scope->currentData().toInt() == AllPackages)
			searchAllPackages(); });
	connect(m_search, &QPushButton::clicked, this, &PackagesPage::searchAllPackages);
	connect(m_scope, &QComboBox::currentIndexChanged, this, &PackagesPage::updateScope);
	connect(m_results, &QTreeWidget::itemActivated, this, [this](QTreeWidgetItem *item)
			{
		const QString file = item->data(0, ResultFileRole).toString();
		if (!file.isEmpty())
			openEntry(file, item->data(0, ResultEntryRole).toString()); });
	connect(m_filterTimer, &QTimer::timeout, this, &PackagesPage::applyFilter);
	connect(m_tree, &QTreeWidget::currentItemChanged, this, &PackagesPage::showPreview);
	connect(m_tree, &QTreeWidget::itemSelectionChanged, this, &PackagesPage::updateButtons);
	connect(m_tree, &QTreeWidget::itemActivated, this, &PackagesPage::openItem);
	connect(m_outputBrowse, &QPushButton::clicked, this, [this]
			{
		const QString dir = QFileDialog::getExistingDirectory(this, tr("Select output folder"), m_outputPath->text());
		if (!dir.isEmpty())
			m_outputPath->setText(dir); });
	connect(m_outputMode, &QComboBox::currentIndexChanged, this, [this]
			{
		const bool workFolder = m_outputMode->currentData().toInt() == WorkFolder;
		m_outputPath->setVisible(workFolder);
		m_outputBrowse->setVisible(workFolder); });
	emit m_outputMode->currentIndexChanged(m_outputMode->currentIndex());
	connect(m_unpackAll, &QPushButton::clicked, this, [this]
			{ unpack(false); });
	connect(m_unpackSelected, &QPushButton::clicked, this, [this]
			{ unpack(true); });

	updateScope();
	loadPackage();
	showPreview();
}

PackagesPage::~PackagesPage()
{
	saveSettings();
	// A running unpack thread must not outlive the page that owns it.
	for (auto *job : findChildren<package::ExtractJob *>())
	{
		job->disconnect(this);
		job->requestInterruption();
		job->wait();
	}
}

void PackagesPage::openPackage(const QString &file)
{
	m_file->setText(file);
	loadPackage();
}

void PackagesPage::openEntry(const QString &file, const QString &entryPath)
{
	m_scope->setCurrentIndex(m_scope->findData(ThisPackage));
	m_filter->clear();
	openPackage(file);
	// Run the cleared filter now: the delayed run would collapse the path opened below.
	m_filterTimer->stop();
	applyFilter();
	const QString wanted = entryPath.toLower();
	std::function<QTreeWidgetItem *(QTreeWidgetItem *)> find = [&](QTreeWidgetItem *parent) -> QTreeWidgetItem *
	{
		for (int i = 0; i < parent->childCount(); ++i)
		{
			QTreeWidgetItem *child = parent->child(i);
			const QString path = child->data(0, PathRole).toString();
			if (path == wanted)
				return child;
			if (wanted.startsWith(path + u'/'))
				return find(child);
		}
		return nullptr;
	};
	if (QTreeWidgetItem *item = find(m_tree->invisibleRootItem()))
	{
		for (QTreeWidgetItem *parent = item->parent(); parent; parent = parent->parent())
			parent->setExpanded(true);
		m_tree->setCurrentItem(item);
		m_tree->scrollToItem(item, QAbstractItemView::PositionAtCenter);
	}
}

void PackagesPage::updateScope()
{
	const bool all = m_scope->currentData().toInt() == AllPackages;
	m_left->setCurrentWidget(all ? m_results : m_tree);
	m_search->setVisible(all);
	m_filter->setPlaceholderText(all ? tr("File name or path, then Enter") : tr("Search files"));
	if (all)
	{
		m_previewMessage->setText(tr("Double-click a result to open its package on it."));
		m_preview->setCurrentWidget(m_previewMessage);
	}
	else
	{
		applyFilter();
		showPreview();
	}
}

void PackagesPage::searchAllPackages()
{
	const QString needle = m_filter->text().trimmed().toLower().replace(u'\\', u'/');
	const auto target = m_games->current();
	if (needle.size() < 2 || !target)
		return;
	if (m_searchTask && m_searchTask->isRunning())
		m_searchTask->cancel();

	m_results->clear();
	m_resultCount = 0;
	Task *task = m_tasks->start(tr("Search \"%1\" in %2").arg(needle, target->app.name));
	m_searchTask = task;
	task->setProgress(0);

	auto cancelled = std::make_shared<std::atomic_bool>(false);
	task->setCanceller([cancelled]
					   { *cancelled = true; });
	const GameList::Target game = *target;
	const QStringList libraries = m_games->libraries();
	QPointer<PackagesPage> page(this);
	QPointer<Task> taskRef(task);

	// Reading every package table takes a while for big games, so it runs off the UI thread.
	QThread *thread = QThread::create([page, taskRef, cancelled, game, libraries, needle]
									  {
		const QStringList packages = gamePackages(game, libraries);
		int found = 0;
		for (int i = 0; i < packages.size() && !*cancelled && found < MaxSearchResults; ++i)
		{
			const auto package = package::Package::open(packages[i]);
			QList<QPair<QString, quint64>> matches;
			if (package)
			{
				for (const package::Entry &entry : package->entries())
				{
					if (entry.path.toLower().contains(needle) && found < MaxSearchResults)
					{
						matches.append({entry.path, entry.size});
						++found;
					}
				}
			}
			const QString file = packages[i];
			const int percent = (i + 1) * 100 / int(packages.size());
			QMetaObject::invokeMethod(qApp, [page, taskRef, file, matches, percent]
									  {
				if (taskRef)
					taskRef->setProgress(percent);
				if (page && !matches.isEmpty())
					page->addResults(file, matches); });
		}
		const bool stopped = *cancelled;
		const int total = int(packages.size());
		QMetaObject::invokeMethod(qApp, [taskRef, found, total, stopped]
								  {
			if (!taskRef)
				return;
			taskRef->appendLine(QCoreApplication::translate("PackagesPage", "%1 match(es) in %2 package(s).").arg(found).arg(total));
			if (found >= MaxSearchResults)
				taskRef->appendLine(QCoreApplication::translate("PackagesPage", "Stopped at %1 matches; narrow the search.").arg(MaxSearchResults));
			taskRef->finish(stopped ? Task::State::Cancelled : Task::State::Succeeded); }); });
	connect(thread, &QThread::finished, thread, &QObject::deleteLater);
	task->appendLine(tr("Looking in the game's VPKs, its addons folder and its workshop downloads…"));
	thread->start();
}

void PackagesPage::addResults(const QString &package, const QList<QPair<QString, quint64>> &matches)
{
	const auto target = m_games->current();
	const QLocale locale;
	auto *group = new QTreeWidgetItem(m_results);
	QString label = package;
	if (target && package.startsWith(target->app.installPath + u'/'))
		label = package.mid(target->app.installPath.size() + 1);
	group->setText(0, tr("%1 (%2)").arg(label).arg(matches.size()));
	group->setToolTip(0, package);
	group->setIcon(0, QIcon::fromTheme("package-x-generic"));
	for (const auto &[path, size] : matches)
	{
		auto *item = new QTreeWidgetItem(group);
		item->setText(0, path);
		item->setText(1, locale.formattedDataSize(qint64(size)));
		item->setTextAlignment(1, Qt::AlignRight | Qt::AlignVCenter);
		item->setData(0, ResultFileRole, package);
		item->setData(0, ResultEntryRole, path);
	}
	group->setExpanded(m_results->topLevelItemCount() <= 20);
	m_resultCount += int(matches.size());
}

void PackagesPage::loadPackage()
{
	const QString file = m_file->text().trimmed();
	if (m_package && m_package->file() == file)
		return;

	m_tree->clear();
	m_package.reset();
	m_info->clear();
	setNotice({});
	showPreview();
	updateButtons();
	if (file.isEmpty())
		return;

	QString error;
	m_package = package::Package::open(file, &error);
	if (!m_package)
	{
		setNotice(tr("ERROR: %1").arg(error));
		return;
	}
	rememberPath("package", file);

	const QLocale locale;
	QHash<QString, QTreeWidgetItem *> folders;
	const auto folderItem = [&](const QString &path, auto &&self) -> QTreeWidgetItem *
	{
		if (path.isEmpty())
			return m_tree->invisibleRootItem();
		if (QTreeWidgetItem *existing = folders.value(path))
			return existing;
		const qsizetype slash = path.lastIndexOf(u'/');
		auto *item = new QTreeWidgetItem(self(slash < 0 ? QString() : path.left(slash), self));
		item->setText(0, path.mid(slash + 1));
		item->setData(0, EntryIndexRole, -1);
		item->setData(0, PathRole, path.toLower());
		folders.insert(path, item);
		return item;
	};

	quint64 totalSize = 0;
	m_tree->setUpdatesEnabled(false);
	const QList<package::Entry> &entries = m_package->entries();
	for (int i = 0; i < entries.size(); ++i)
	{
		const package::Entry &entry = entries[i];
		const qsizetype slash = entry.path.lastIndexOf(u'/');
		auto *item = new QTreeWidgetItem(folderItem(slash < 0 ? QString() : entry.path.left(slash), folderItem));
		item->setText(0, entry.path.mid(slash + 1));
		item->setText(1, locale.formattedDataSize(qint64(entry.size)));
		item->setTextAlignment(1, Qt::AlignRight | Qt::AlignVCenter);
		item->setData(0, EntryIndexRole, i);
		item->setData(0, PathRole, entry.path.toLower());
		totalSize += entry.size;
	}
	m_tree->sortItems(0, Qt::AscendingOrder);
	m_tree->setUpdatesEnabled(true);

	QString info = tr("%1, %n file(s), %2", nullptr, int(entries.size()))
					   .arg(m_package->format(), locale.formattedDataSize(qint64(totalSize)));
	if (!m_package->title().isEmpty())
		info = m_package->title() + " — " + info;
	m_info->setText(info);
	applyFilter();
	showPreview();
	updateButtons();
}

void PackagesPage::applyFilter()
{
	const QString needle = m_filter->text().trimmed().toLower();
	// Shows an item when its path matches, or when any item under it does.
	const std::function<bool(QTreeWidgetItem *)> visit = [&](QTreeWidgetItem *item) -> bool
	{
		bool anyChild = false;
		for (int i = 0; i < item->childCount(); ++i)
			anyChild = visit(item->child(i)) || anyChild;
		const bool self = needle.isEmpty() || item->data(0, PathRole).toString().contains(needle);
		const bool visible = self || anyChild;
		item->setHidden(!visible);
		if (!needle.isEmpty() && item->childCount() > 0)
			item->setExpanded(anyChild);
		return visible;
	};
	m_tree->setUpdatesEnabled(false);
	for (int i = 0; i < m_tree->topLevelItemCount(); ++i)
		visit(m_tree->topLevelItem(i));
	if (needle.isEmpty())
		m_tree->collapseAll();
	m_tree->setUpdatesEnabled(true);
}

void PackagesPage::showPreview()
{
	QTreeWidgetItem *item = m_tree->currentItem();
	const int index = item ? item->data(0, EntryIndexRole).toInt() : -1;
	if (!m_package || index < 0)
	{
		m_previewMessage->setText(m_package ? tr("Select a file to preview it.") : tr("Open a package, or drop one onto the window."));
		m_preview->setCurrentWidget(m_previewMessage);
		return;
	}

	const package::Entry &entry = m_package->entries().at(index);
	const bool text = isTextEntry(entry.path);
	const QByteArray format = QFileInfo(entry.path).suffix().toLower().toLatin1();
	const bool image = QImageReader::supportedImageFormats().contains(format);
	if (!text && !image)
	{
		m_previewMessage->setText(tr("No preview for this file type. Double-click to open it."));
		m_preview->setCurrentWidget(m_previewMessage);
		return;
	}

	QString error;
	const auto data = m_package->read(entry, MaxPreviewSize, &error);
	if (!data)
	{
		m_previewMessage->setText(error);
		m_preview->setCurrentWidget(m_previewMessage);
		return;
	}
	if (text)
	{
		m_previewText->setPlainText(QString::fromUtf8(*data));
		m_preview->setCurrentWidget(m_previewText);
		return;
	}
	const QImage picture = QImage::fromData(*data, format.constData());
	if (picture.isNull())
	{
		m_previewMessage->setText(tr("This image could not be read."));
		m_preview->setCurrentWidget(m_previewMessage);
		return;
	}
	m_previewImage->setPixmap(QPixmap::fromImage(picture));
	m_preview->setCurrentWidget(m_previewImageArea);
}

QStringList PackagesPage::extractToTemp(const QList<int> &indexes)
{
	QStringList files;
	const QString temp = tempPath("packages");
	if (!m_package || temp.isEmpty())
		return files;
	const QString dir = temp + u'/' + QFileInfo(m_package->file()).completeBaseName();
	for (int index : indexes)
	{
		const package::Entry &entry = m_package->entries().at(index);
		if (m_package->extract(entry, dir))
			files << dir + u'/' + entry.path;
	}
	return files;
}

void PackagesPage::openItem(QTreeWidgetItem *item)
{
	const int index = item->data(0, EntryIndexRole).toInt();
	if (!m_package || index < 0)
		return;
	// Removed together with the page, like Crowbar's temporary preview copies.
	const QStringList files = extractToTemp({index});
	if (files.isEmpty())
	{
		setNotice(tr("ERROR: Could not open \"%1\".").arg(m_package->entries().at(index).path));
		return;
	}
	if (!QDesktopServices::openUrl(QUrl::fromLocalFile(files.first())))
		setNotice(tr("No application is associated with \"%1\". Temporary copy: %2").arg(QFileInfo(files.first()).fileName(), files.first()));
}

QString PackagesPage::outputDir() const
{
	if (m_outputMode->currentData().toInt() == WorkFolder)
		return m_outputPath->text().trimmed();
	if (!m_package)
		return {};
	const QFileInfo file(m_package->file());
	QString name = file.completeBaseName();
	if (name.endsWith("_dir", Qt::CaseInsensitive))
		name.chop(4);
	return file.absolutePath() + u'/' + name;
}

QList<int> PackagesPage::selectedEntries() const
{
	QSet<int> selected;
	for (const QTreeWidgetItem *item : m_tree->selectedItems())
		collectEntries(item, selected);
	QList<int> indexes(selected.begin(), selected.end());
	std::sort(indexes.begin(), indexes.end());
	return indexes;
}

void PackagesPage::unpack(bool selectedOnly)
{
	loadPackage();
	if (!m_package || m_unpacking)
		return;
	saveSettings();

	QList<int> indexes;
	if (selectedOnly)
	{
		indexes = selectedEntries();
	}
	else
	{
		for (int i = 0; i < m_package->entries().size(); ++i)
			indexes << i;
	}

	const QString output = outputDir();
	Task *task = m_tasks->start(tr("Unpack %1").arg(QFileInfo(m_package->file()).fileName()));
	if (output.isEmpty())
	{
		task->fail(tr("Select an output folder."));
		return;
	}
	if (indexes.isEmpty())
	{
		task->fail(tr("Nothing is selected."));
		return;
	}

	task->appendLine(tr("Unpacking %n file(s) to \"%1\"…", nullptr, int(indexes.size())).arg(output));
	task->setProgress(0);
	auto *job = new package::ExtractJob(*m_package, indexes, output, this);
	connect(job, &package::ExtractJob::progress, task, [task](int done, int total)
			{ task->setProgress(total > 0 ? done * 100 / total : 0); });
	connect(job, &package::ExtractJob::message, task, &Task::appendLine);
	connect(job, &package::ExtractJob::done, this, [this, task, job, output](int extracted, int failed, bool cancelled)
			{
		task->appendLine(cancelled ? tr("Cancelled after %1 file(s).").arg(extracted)
								   : tr("Unpacked %1 file(s), %2 failed.").arg(extracted).arg(failed));
		if (!cancelled)
			task->appendLine(output);
		task->finish(cancelled ? Task::State::Cancelled : failed ? Task::State::Failed : Task::State::Succeeded);
		job->wait();
		job->deleteLater();
		m_unpacking = false;
		updateButtons(); });
	task->setCanceller([job]
					   { job->requestInterruption(); });
	m_unpacking = true;
	updateButtons();
	job->start();
}

void PackagesPage::updateButtons()
{
	const bool ready = m_package.has_value() && !m_unpacking;
	m_unpackAll->setEnabled(ready);
	m_unpackSelected->setEnabled(ready && !m_tree->selectedItems().isEmpty());
}

void PackagesPage::saveSettings() const
{
	rememberPath("unpack-output", m_outputPath->text());
	QSettings settings;
	settings.setValue("unpack/file", m_file->text());
	settings.setValue("unpack/outputMode", m_outputMode->currentIndex());
	settings.setValue("unpack/outputPath", m_outputPath->text());
}
