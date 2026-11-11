#include "ui/DownloadPage.h"

#include "ui/GameList.h"
#include "ui/PathField.h"
#include "ui/Tasks.h"
#include "ui/TempDir.h"

#include <QCheckBox>
#include <QDesktopServices>
#include <QDir>
#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHash>
#include <QMap>
#include <QImage>
#include <QLabel>
#include <QLineEdit>
#include <QLocale>
#include <QPixmap>
#include <QPushButton>
#include <QSettings>
#include <QStandardPaths>
#include <QUrl>
#include <QVBoxLayout>

#include <memory>

namespace
{

	constexpr int PreviewSize = 128;

	// Kept between runs, so steamcmd does not download and update itself every time.
	QString steamCmdHome()
	{
		return QStandardPaths::writableLocation(QStandardPaths::AppDataLocation) + "/steamcmd-home";
	}

} // namespace

DownloadPage::DownloadPage(GameList *games, TaskManager *tasks, QWidget *parent)
	: Page(tr("Download"), games, tasks, parent), m_client(new workshop::Client(this)), m_item(new QLineEdit(this)), m_preview(new QLabel(this)),
	  m_info(new QLabel(this)), m_outputPath(new QLineEdit(this)), m_withTitle(new QCheckBox(tr("Title in file name"), this)),
	  m_convert(new QCheckBox(tr("Unpack old .bin files to .gma"), this)), m_download(new QPushButton(tr("Download"), this))
{
	setUpPathField(m_outputPath, "download-output", PathKind::Folder);

	const QSettings settings;
	m_item->setPlaceholderText(tr("Workshop item link or ID"));
	m_item->setClearButtonEnabled(true);
	m_outputPath->setText(settings.value("download/outputPath", QDir::homePath() + "/Downloads").toString());
	m_withTitle->setChecked(settings.value("download/withTitle", true).toBool());
	m_convert->setChecked(settings.value("download/convert", true).toBool());
	m_preview->setFixedSize(PreviewSize, PreviewSize);
	m_preview->setAlignment(Qt::AlignCenter);
	m_preview->setFrameShape(QFrame::StyledPanel);
	m_info->setWordWrap(true);
	m_info->setTextInteractionFlags(Qt::TextSelectableByMouse);
	m_info->setAlignment(Qt::AlignTop | Qt::AlignLeft);

	auto *getInfo = new QPushButton(tr("Get Info"), this);
	auto *openPage = new QPushButton(QIcon::fromTheme("internet-web-browser"), tr("Open Workshop Page"), this);
	auto *itemRow = new QHBoxLayout;
	itemRow->addWidget(m_item, 1);
	itemRow->addWidget(getInfo);
	itemRow->addWidget(openPage);

	auto *card = new QHBoxLayout;
	card->addWidget(m_preview);
	card->addWidget(m_info, 1);

	auto *browse = new QPushButton(tr("Browse…"), this);
	auto *outputRow = new QHBoxLayout;
	outputRow->addWidget(m_outputPath, 1);
	outputRow->addWidget(browse);

	auto *optionsRow = new QHBoxLayout;
	optionsRow->addWidget(m_withTitle);
	optionsRow->addWidget(m_convert);
	optionsRow->addStretch(1);

	auto *form = new QFormLayout;
	form->addRow(tr("Item:"), itemRow);
	form->addRow(QString(), card);
	form->addRow(tr("Output to:"), outputRow);
	form->addRow(tr("Options:"), optionsRow);

	auto *buttons = new QHBoxLayout;
	buttons->addStretch(1);
	buttons->addWidget(m_download);
	m_download->setDefault(true);

	content()->addLayout(form);
	content()->addLayout(buttons);
	content()->addStretch(1);

	connect(getInfo, &QPushButton::clicked, this, &DownloadPage::showInfo);
	connect(m_item, &QLineEdit::returnPressed, this, &DownloadPage::showInfo);
	connect(m_download, &QPushButton::clicked, this, &DownloadPage::startDownloads);
	connect(openPage, &QPushButton::clicked, this, [this]
			{
		const QList<quint64> ids = workshop::parseItemIds(m_item->text());
		if (!ids.isEmpty())
			QDesktopServices::openUrl(QUrl(workshop::itemUrl(ids.first()))); });
	connect(browse, &QPushButton::clicked, this, [this]
			{
		const QString dir = QFileDialog::getExistingDirectory(this, tr("Select output folder"), m_outputPath->text());
		if (!dir.isEmpty())
			m_outputPath->setText(dir); });
	connect(m_client, &workshop::Client::dataReady, this, [this](quint64 id, const QByteArray &data)
			{
		const QImage image = QImage::fromData(data);
		if (id == m_shownId && !image.isNull())
			m_preview->setPixmap(QPixmap::fromImage(image.scaled(PreviewSize, PreviewSize, Qt::KeepAspectRatio, Qt::SmoothTransformation))); });

	m_item->setPlaceholderText(tr("Workshop links or IDs; collections are expanded"));
	m_info->setText(tr("Enter one or more workshop links or item IDs, or a collection."));
}

DownloadPage::~DownloadPage()
{
	saveSettings();
}

void DownloadPage::showInfo()
{
	const QList<quint64> ids = workshop::parseItemIds(m_item->text());
	m_preview->clear();
	m_shownId = 0;
	if (ids.isEmpty())
	{
		m_info->setText(tr("Enter one or more workshop links or item IDs, or a collection."));
		return;
	}
	if (ids.size() > 1)
	{
		m_info->setText(tr("%n items or collections; Download fetches them one after another.", nullptr, int(ids.size())));
		return;
	}
	m_info->setText(tr("Getting item details…"));
	// One lookup per request, so a slow answer cannot overwrite a newer one.
	auto *client = new workshop::Client(this);
	auto collectionSize = std::make_shared<int>(-1);
	connect(client, &workshop::Client::collectionReady, this, [client, collectionSize](quint64 id, const QList<quint64> &children, bool isCollection)
			{
		if (isCollection)
			*collectionSize = int(children.size());
		client->fetchDetails(id); });
	connect(client, &workshop::Client::detailsReady, this, [this, client, collectionSize](const workshop::ItemDetails &details)
			{
		client->deleteLater();
		if (workshop::parseItemIds(m_item->text()) == QList<quint64>{details.id})
			showDetails(details, *collectionSize); });
	client->fetchCollection(ids.first());
}

void DownloadPage::showDetails(const workshop::ItemDetails &details, int collectionSize)
{
	if (!details.isValid())
	{
		m_info->setText(tr("ERROR: %1").arg(details.error));
		return;
	}
	m_shownId = details.id;
	if (!details.previewUrl.isEmpty())
		m_client->fetchData(details.previewUrl, details.id);

	QString game = QString::number(details.appId);
	for (const steam::App &app : m_games->apps())
	{
		if (app.id == details.appId && !app.isMod)
			game = app.name;
	}
	const QLocale locale;
	if (collectionSize >= 0)
	{
		m_info->setText(tr("<b>%1</b><br>Collection for %2 · %n item(s) · updated %3", nullptr, collectionSize)
							.arg(details.title.toHtmlEscaped(), game.toHtmlEscaped(), locale.toString(details.updated, QLocale::ShortFormat)));
		return;
	}
	const bool cached = !workshop::findCachedFiles(m_games->libraries(), details.appId, details.id).isEmpty();
	const QString source = cached						  ? tr("in Steam's workshop cache")
						   : !details.fileUrl.isEmpty()		  ? tr("direct download")
						   : !workshop::findSteamCmd().isEmpty() ? tr("via steamcmd")
															   : tr("not available: subscribe in Steam or install steamcmd");
	m_info->setText(tr("<b>%1</b><br>%2 · %3 · updated %4 · %5")
						.arg(details.title.toHtmlEscaped(), game.toHtmlEscaped(), locale.formattedDataSize(details.fileSize),
							 locale.toString(details.updated, QLocale::ShortFormat), source));
}

void DownloadPage::startDownloads()
{
	const QList<quint64> ids = workshop::parseItemIds(m_item->text());
	if (ids.isEmpty() || m_resolving)
	{
		if (ids.isEmpty())
			m_info->setText(tr("Enter one or more workshop links or item IDs, or a collection."));
		return;
	}
	saveSettings();
	m_resolving = true;
	m_download->setEnabled(false);

	// Collections answer in any order; keep the items in the order they were entered.
	auto *client = new workshop::Client(this);
	auto expanded = std::make_shared<QMap<int, QList<quint64>>>();
	auto pending = std::make_shared<int>(int(ids.size()));
	auto batches = std::make_shared<int>(0);
	auto found = std::make_shared<QHash<quint64, workshop::ItemDetails>>();
	auto order = std::make_shared<QList<quint64>>();

	const auto finish = [this, client, found, order]
	{
		for (quint64 id : std::as_const(*order))
		{
			const workshop::ItemDetails details = found->value(id);
			if (details.isValid())
			{
				m_queue << details;
			}
			else
			{
				Task *task = m_tasks->start(tr("Download %1").arg(id));
				task->fail(details.error.isEmpty() ? tr("Item %1 was not found, or it is private or removed.").arg(id) : details.error);
			}
		}
		client->deleteLater();
		m_resolving = false;
		m_download->setEnabled(true);
		startNext();
	};

	connect(client, &workshop::Client::detailsListReady, this, [found, batches, finish](const QList<workshop::ItemDetails> &list)
			{
		for (const workshop::ItemDetails &details : list)
			found->insert(details.id, details);
		if (--*batches == 0)
			finish(); });
	connect(client, &workshop::Client::collectionReady, this, [this, client, ids, expanded, pending, batches, order, finish](quint64 id, const QList<quint64> &children, bool isCollection)
			{
		expanded->insert(int(ids.indexOf(id)), isCollection ? children : QList<quint64>{id});
		if (--*pending > 0)
			return;
		for (const QList<quint64> &items : std::as_const(*expanded))
		{
			for (quint64 item : items)
			{
				if (!order->contains(item))
					*order << item;
			}
		}
		if (order->isEmpty())
		{
			finish();
			return;
		}
		// The details API is asked in batches to keep each request small.
		constexpr int BatchSize = 100;
		*batches = int((order->size() + BatchSize - 1) / BatchSize);
		for (int i = 0; i < order->size(); i += BatchSize)
			client->fetchDetailsList(order->mid(i, BatchSize)); });
	for (quint64 id : ids)
		client->fetchCollection(id);
}

void DownloadPage::startNext()
{
	if (m_current || m_queue.isEmpty())
		return;
	download(m_queue.takeFirst());
}

void DownloadPage::download(const workshop::ItemDetails &details)
{
	Task *task = m_tasks->start(tr("Download %1").arg(details.title.isEmpty() ? QString::number(details.id) : details.title));
	m_current = task;
	// steamcmd allows one run at a time, and Steam's servers prefer it too.
	connect(task, &Task::stateChanged, this, [this]
			{
		m_current = nullptr;
		startNext(); });
	if (m_outputPath->text().trimmed().isEmpty())
	{
		task->fail(tr("Select an output folder."));
		return;
	}

	// Steam's own copy is the newest the client has; it needs no network at all.
	const QStringList cached = workshop::findCachedFiles(m_games->libraries(), details.appId, details.id);
	if (!cached.isEmpty())
	{
		task->appendLine(tr("Copying from Steam's workshop cache: %1").arg(QFileInfo(cached.first()).absolutePath()));
		save(task, details, cached);
		return;
	}

	if (!details.fileUrl.isEmpty())
	{
		auto *client = new workshop::Client(task);
		const QString file = tempPath("downloads") + QStringLiteral("/%1.download").arg(details.id);
		task->appendLine(tr("Downloading %1").arg(details.fileUrl));
		task->setProgress(0);
		connect(client, &workshop::Client::progress, task, [task](qint64 received, qint64 total)
				{
			if (total > 0)
				task->setProgress(int(received * 100 / total)); });
		connect(client, &workshop::Client::downloaded, this, [this, task, details, file](bool ok, const QString &error)
				{
			if (!ok)
			{
				task->fail(tr("Download failed: %1").arg(error));
				return;
			}
			save(task, details, {file}); });
		task->setCanceller([client]
						   { client->abort(); });
		client->download(details.fileUrl, file);
		return;
	}

	const QString steamCmd = workshop::findSteamCmd();
	if (steamCmd.isEmpty())
	{
		task->fail(tr("This item has no direct download link. Subscribe to it in Steam (it will then be copied from Steam's cache), or install steamcmd."));
		return;
	}
	auto downloaded = std::make_shared<QString>();
	connect(task, &Task::lineAdded, this, [downloaded](const QString &line)
			{
		const QString found = workshop::parseSteamCmdPath(line);
		if (!found.isEmpty())
			*downloaded = found; });
	task->run(workshop::steamCmdCommand(steamCmd, steamCmdHome(), tempPath("steamcmd"), details.appId, details.id),
			  [this, task, details, downloaded](int, bool)
			  {
		// steamcmd names the file for single-file items and the folder otherwise.
		QStringList files;
		if (QFileInfo(*downloaded).isFile())
			files << *downloaded;
		for (const QFileInfo &file : QDir(*downloaded).entryInfoList(QDir::Files, QDir::Name))
			files << file.absoluteFilePath();
		if (files.isEmpty())
		{
			task->appendLine(tr("ERROR: steamcmd did not download the item. Anonymous downloads only work for games that allow them; subscribe in Steam instead."));
			return false;
		}
		save(task, details, files);
		return task->state() == Task::State::Succeeded; });
}

void DownloadPage::save(Task *task, const workshop::ItemDetails &details, const QStringList &files)
{
	const QString output = m_outputPath->text().trimmed();
	const QString baseName = workshop::fileBaseName(details, m_withTitle->isChecked());
	bool ok = true;
	for (int i = 0; i < files.size(); ++i)
	{
		const QString name = files.size() == 1 ? baseName : QStringLiteral("%1_%2").arg(baseName).arg(i + 1);
		QString error;
		const QString saved = workshop::saveFile(files[i], output, name, m_convert->isChecked(), &error);
		task->appendLine(saved.isEmpty() ? tr("ERROR: %1").arg(error) : tr("Saved \"%1\".").arg(saved));
		ok = ok && !saved.isEmpty();
		// Downloads can be gigabytes; drop our own copy now rather than at exit.
		if (!saved.isEmpty() && files[i].startsWith(tempPath() + u'/'))
			QFile::remove(files[i]);
	}
	task->finish(ok ? Task::State::Succeeded : Task::State::Failed);
}

void DownloadPage::saveSettings() const
{
	rememberPath("download-output", m_outputPath->text());
	QSettings settings;
	settings.setValue("download/outputPath", m_outputPath->text());
	settings.setValue("download/withTitle", m_withTitle->isChecked());
	settings.setValue("download/convert", m_convert->isChecked());
}
