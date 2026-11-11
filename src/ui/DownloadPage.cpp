#include "ui/DownloadPage.h"

#include "ui/GameList.h"
#include "ui/PathField.h"
#include "ui/Tasks.h"

#include <QCheckBox>
#include <QDesktopServices>
#include <QDir>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QImage>
#include <QLabel>
#include <QLineEdit>
#include <QLocale>
#include <QPixmap>
#include <QPushButton>
#include <QSettings>
#include <QUrl>
#include <QVBoxLayout>

#include <memory>

namespace
{

	constexpr int PreviewSize = 128;

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

	connect(getInfo, &QPushButton::clicked, this, [this]
			{ fetch(false); });
	connect(m_item, &QLineEdit::returnPressed, this, [this]
			{ fetch(false); });
	connect(m_download, &QPushButton::clicked, this, [this]
			{
		if (m_details.isValid() && m_details.id == workshop::parseItemId(m_item->text()))
			download();
		else
			fetch(true); });
	connect(openPage, &QPushButton::clicked, this, [this]
			{
		if (const quint64 id = workshop::parseItemId(m_item->text()))
			QDesktopServices::openUrl(QUrl(workshop::itemUrl(id))); });
	connect(browse, &QPushButton::clicked, this, [this]
			{
		const QString dir = QFileDialog::getExistingDirectory(this, tr("Select output folder"), m_outputPath->text());
		if (!dir.isEmpty())
			m_outputPath->setText(dir); });
	connect(m_client, &workshop::Client::detailsReady, this, &DownloadPage::showDetails);
	connect(m_client, &workshop::Client::dataReady, this, [this](quint64 id, const QByteArray &data)
			{
		const QImage image = QImage::fromData(data);
		if (id == m_details.id && !image.isNull())
			m_preview->setPixmap(QPixmap::fromImage(image.scaled(PreviewSize, PreviewSize, Qt::KeepAspectRatio, Qt::SmoothTransformation))); });

	m_info->setText(tr("Enter a workshop link or a numeric item ID."));
}

DownloadPage::~DownloadPage()
{
	saveSettings();
}

void DownloadPage::fetch(bool thenDownload)
{
	const quint64 id = workshop::parseItemId(m_item->text());
	if (id == 0)
	{
		m_info->setText(tr("Enter a workshop link or a numeric item ID."));
		return;
	}
	m_downloadAfterFetch = thenDownload;
	m_preview->clear();
	m_info->setText(tr("Getting item details…"));
	m_client->fetchDetails(id);
}

void DownloadPage::showDetails(const workshop::ItemDetails &details)
{
	m_details = details;
	if (!details.isValid())
	{
		m_info->setText(tr("ERROR: %1").arg(details.error));
		return;
	}

	QString game = QString::number(details.appId);
	for (const steam::App &app : m_games->apps())
	{
		if (app.id == details.appId)
			game = app.name;
	}
	const QLocale locale;
	const bool cached = !workshop::findCachedFiles(m_games->libraries(), details.appId, details.id).isEmpty();
	const QString source = cached						  ? tr("in Steam's workshop cache")
						   : !details.fileUrl.isEmpty()		  ? tr("direct download")
						   : !workshop::findSteamCmd().isEmpty() ? tr("via steamcmd")
															   : tr("not available: subscribe in Steam or install steamcmd");
	m_info->setText(tr("<b>%1</b><br>%2 · %3 · updated %4 · %5")
						.arg(details.title.toHtmlEscaped(), game.toHtmlEscaped(), locale.formattedDataSize(details.fileSize),
							 locale.toString(details.updated, QLocale::ShortFormat), source));
	if (!details.previewUrl.isEmpty())
		m_client->fetchData(details.previewUrl, details.id);
	if (m_downloadAfterFetch)
		download();
}

void DownloadPage::download()
{
	saveSettings();
	const workshop::ItemDetails details = m_details;
	Task *task = m_tasks->start(tr("Download %1").arg(details.title.isEmpty() ? QString::number(details.id) : details.title));
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
		save(task, cached);
		return;
	}

	if (!details.fileUrl.isEmpty())
	{
		// One client per download, so several can run side by side.
		auto *client = new workshop::Client(task);
		const QString file = m_tempDir.filePath(QStringLiteral("%1.download").arg(details.id));
		task->appendLine(tr("Downloading %1").arg(details.fileUrl));
		task->setProgress(0);
		connect(client, &workshop::Client::progress, task, [task](qint64 received, qint64 total)
				{
			if (total > 0)
				task->setProgress(int(received * 100 / total)); });
		connect(client, &workshop::Client::downloaded, this, [this, task, file](bool ok, const QString &error)
				{
			if (!ok)
			{
				task->fail(tr("Download failed: %1").arg(error));
				return;
			}
			save(task, {file}); });
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
	auto folder = std::make_shared<QString>();
	connect(task, &Task::lineAdded, this, [folder](const QString &line)
			{
		const QString found = workshop::parseSteamCmdFolder(line);
		if (!found.isEmpty())
			*folder = found; });
	task->run(workshop::steamCmdCommand(steamCmd, m_tempDir.filePath("steamcmd"), details.appId, details.id),
			  [this, task, folder](int, bool)
			  {
		QStringList files;
		for (const QFileInfo &file : QDir(*folder).entryInfoList(QDir::Files, QDir::Name))
			files << file.absoluteFilePath();
		if (files.isEmpty())
		{
			task->appendLine(tr("ERROR: steamcmd did not download the item. Anonymous downloads only work for games that allow them; subscribe in Steam instead."));
			return false;
		}
		save(task, files);
		return task->state() == Task::State::Succeeded; });
}

void DownloadPage::save(Task *task, const QStringList &files)
{
	const QString output = m_outputPath->text().trimmed();
	const QString baseName = workshop::fileBaseName(m_details, m_withTitle->isChecked());
	bool ok = true;
	for (int i = 0; i < files.size(); ++i)
	{
		const QString name = files.size() == 1 ? baseName : QStringLiteral("%1_%2").arg(baseName).arg(i + 1);
		QString error;
		const QString saved = workshop::saveFile(files[i], output, name, m_convert->isChecked(), &error);
		task->appendLine(saved.isEmpty() ? tr("ERROR: %1").arg(error) : tr("Saved \"%1\".").arg(saved));
		ok = ok && !saved.isEmpty();
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
