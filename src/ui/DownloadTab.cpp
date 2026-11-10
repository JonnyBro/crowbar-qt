#include "ui/DownloadTab.h"

#include "core/ToolProcess.h"
#include "ui/GameList.h"

#include <QCheckBox>
#include <QDesktopServices>
#include <QDir>
#include <QFileDialog>
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QLocale>
#include <QPlainTextEdit>
#include <QProgressBar>
#include <QPushButton>
#include <QSettings>
#include <QUrl>
#include <QVBoxLayout>

DownloadTab::DownloadTab(GameList *games, QWidget *parent)
    : QWidget(parent)
    , m_games(games)
    , m_client(new workshop::Client(this))
    , m_steamCmd(new ToolProcess(this))
    , m_item(new QLineEdit(this))
    , m_info(new QLabel(this))
    , m_outputPath(new QLineEdit(this))
    , m_withTitle(new QCheckBox(tr("Title in file name"), this))
    , m_convert(new QCheckBox(tr("Unpack old .bin files to .gma"), this))
    , m_download(new QPushButton(tr("Download"), this))
    , m_cancel(new QPushButton(tr("Cancel"), this))
    , m_openPage(new QPushButton(tr("Open Workshop Page"), this))
    , m_progress(new QProgressBar(this))
    , m_log(new QPlainTextEdit(this))
{
    const QSettings settings;
    m_item->setPlaceholderText(tr("Workshop item link or ID"));
    m_outputPath->setText(settings.value("download/outputPath", QDir::homePath() + "/Downloads").toString());
    m_withTitle->setChecked(settings.value("download/withTitle", true).toBool());
    m_convert->setChecked(settings.value("download/convert", true).toBool());
    m_info->setWordWrap(true);
    m_info->setTextInteractionFlags(Qt::TextSelectableByMouse);
    m_progress->setVisible(false);
    m_log->setReadOnly(true);
    m_log->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));

    auto *getInfo = new QPushButton(tr("Get Info"), this);
    auto *itemRow = new QHBoxLayout;
    itemRow->addWidget(m_item, 1);
    itemRow->addWidget(getInfo);
    itemRow->addWidget(m_openPage);

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
    form->addRow(QString(), m_info);
    form->addRow(tr("Output to:"), outputRow);
    form->addRow(tr("Options:"), optionsRow);

    auto *buttons = new QHBoxLayout;
    buttons->addWidget(m_progress, 1);
    buttons->addWidget(m_download);
    buttons->addWidget(m_cancel);

    auto *layout = new QVBoxLayout(this);
    layout->addLayout(form);
    layout->addLayout(buttons);
    layout->addWidget(m_log, 1);

    connect(getInfo, &QPushButton::clicked, this, [this] { fetch(false); });
    connect(m_item, &QLineEdit::returnPressed, this, [this] { fetch(false); });
    connect(m_download, &QPushButton::clicked, this, [this] {
        if (m_details.isValid() && m_details.id == workshop::parseItemId(m_item->text()))
            download();
        else
            fetch(true);
    });
    connect(m_openPage, &QPushButton::clicked, this, [this] {
        if (const quint64 id = workshop::parseItemId(m_item->text()))
            QDesktopServices::openUrl(QUrl(workshop::itemUrl(id)));
    });
    connect(browse, &QPushButton::clicked, this, [this] {
        const QString dir = QFileDialog::getExistingDirectory(this, tr("Select output folder"), m_outputPath->text());
        if (!dir.isEmpty())
            m_outputPath->setText(dir);
    });
    connect(m_cancel, &QPushButton::clicked, this, [this] {
        m_client->abort();
        m_steamCmd->cancel();
    });

    connect(m_client, &workshop::Client::detailsReady, this, &DownloadTab::showDetails);
    connect(m_client, &workshop::Client::progress, this, [this](qint64 received, qint64 total) {
        m_progress->setRange(0, total > 0 ? 1000 : 0);
        if (total > 0)
            m_progress->setValue(int(received * 1000 / total));
    });
    connect(m_client, &workshop::Client::downloaded, this, [this](bool ok, const QString &error) {
        if (!ok) {
            m_log->appendPlainText(tr("ERROR: Download failed: %1").arg(error));
            setBusy(false);
            return;
        }
        finish({m_tempDir.filePath(QStringLiteral("%1.download").arg(m_details.id))});
    });

    connect(m_steamCmd, &ToolProcess::outputLine, this, [this](const QString &line) {
        m_log->appendPlainText(line);
        const QString folder = workshop::parseSteamCmdFolder(line);
        if (!folder.isEmpty())
            m_steamCmdFolder = folder;
    });
    connect(m_steamCmd, &ToolProcess::failedToStart, this, [this](const QString &error) {
        m_log->appendPlainText(tr("ERROR: %1").arg(error));
        setBusy(false);
    });
    connect(m_steamCmd, &ToolProcess::finished, this, [this] {
        QStringList files;
        for (const QFileInfo &file : QDir(m_steamCmdFolder).entryInfoList(QDir::Files, QDir::Name))
            files << file.absoluteFilePath();
        if (files.isEmpty()) {
            m_log->appendPlainText(tr("ERROR: steamcmd did not download the item. Anonymous downloads only work "
                                      "for games that allow them; subscribe in Steam instead."));
            setBusy(false);
            return;
        }
        finish(files);
    });

    setBusy(false);
}

DownloadTab::~DownloadTab()
{
    saveSettings();
}

void DownloadTab::fetch(bool thenDownload)
{
    const quint64 id = workshop::parseItemId(m_item->text());
    if (id == 0) {
        m_info->setText(tr("Enter a workshop link or a numeric item ID."));
        return;
    }
    m_downloadAfterFetch = thenDownload;
    m_info->setText(tr("Getting item details…"));
    setBusy(true);
    m_client->fetchDetails(id);
}

void DownloadTab::showDetails(const workshop::ItemDetails &details)
{
    m_details = details;
    setBusy(false);
    if (!details.isValid()) {
        m_info->setText(tr("ERROR: %1").arg(details.error));
        return;
    }

    QString game = QString::number(details.appId);
    for (const steam::App &app : m_games->apps()) {
        if (app.id == details.appId)
            game = app.name;
    }
    const QLocale locale;
    const bool cached = !workshop::findCachedFiles(m_games->libraries(), details.appId, details.id).isEmpty();
    const QString source = cached ? tr("in Steam's workshop cache")
                         : !details.fileUrl.isEmpty() ? tr("direct download")
                         : !workshop::findSteamCmd().isEmpty() ? tr("via steamcmd")
                                                               : tr("not available: subscribe in Steam or install steamcmd");
    m_info->setText(tr("<b>%1</b><br>%2 · %3 · updated %4 · %5")
                        .arg(details.title.toHtmlEscaped(), game.toHtmlEscaped(),
                             locale.formattedDataSize(details.fileSize),
                             locale.toString(details.updated, QLocale::ShortFormat), source));
    if (m_downloadAfterFetch)
        download();
}

void DownloadTab::download()
{
    saveSettings();
    const QString output = m_outputPath->text().trimmed();
    if (output.isEmpty()) {
        m_log->appendPlainText(tr("ERROR: Select an output folder."));
        return;
    }

    // Steam's own copy is the newest the client has; it needs no network at all.
    const QStringList cached = workshop::findCachedFiles(m_games->libraries(), m_details.appId, m_details.id);
    if (!cached.isEmpty()) {
        m_log->appendPlainText(tr("Copying from Steam's workshop cache: %1").arg(QFileInfo(cached.first()).absolutePath()));
        setBusy(true);
        finish(cached);
        return;
    }

    if (!m_details.fileUrl.isEmpty()) {
        m_log->appendPlainText(tr("Downloading %1").arg(m_details.fileUrl));
        setBusy(true);
        m_progress->setVisible(true);
        m_client->download(m_details.fileUrl, m_tempDir.filePath(QStringLiteral("%1.download").arg(m_details.id)));
        return;
    }

    const QString steamCmd = workshop::findSteamCmd();
    if (!steamCmd.isEmpty()) {
        m_steamCmdFolder.clear();
        const launcher::Command command =
            workshop::steamCmdCommand(steamCmd, m_tempDir.filePath("steamcmd"), m_details.appId, m_details.id);
        m_log->appendPlainText(command.displayString() + '\n');
        setBusy(true);
        m_progress->setVisible(true);
        m_progress->setRange(0, 0);
        m_steamCmd->start(command);
        return;
    }

    m_log->appendPlainText(tr("ERROR: This item has no direct download link. Subscribe to it in Steam (it will then be "
                              "copied from Steam's cache), or install steamcmd."));
}

void DownloadTab::finish(const QStringList &files)
{
    const QString output = m_outputPath->text().trimmed();
    const QString baseName = workshop::fileBaseName(m_details, m_withTitle->isChecked());
    for (int i = 0; i < files.size(); ++i) {
        const QString name = files.size() == 1 ? baseName : QStringLiteral("%1_%2").arg(baseName).arg(i + 1);
        QString error;
        const QString saved = workshop::saveFile(files[i], output, name, m_convert->isChecked(), &error);
        m_log->appendPlainText(saved.isEmpty() ? tr("ERROR: %1").arg(error) : tr("Saved \"%1\".").arg(saved));
    }
    setBusy(false);
}

void DownloadTab::setBusy(bool busy)
{
    m_busy = busy;
    m_download->setEnabled(!busy);
    m_cancel->setEnabled(busy);
    m_item->setEnabled(!busy);
    if (!busy)
        m_progress->setVisible(false);
}

void DownloadTab::saveSettings() const
{
    QSettings settings;
    settings.setValue("download/outputPath", m_outputPath->text());
    settings.setValue("download/withTitle", m_withTitle->isChecked());
    settings.setValue("download/convert", m_convert->isChecked());
}
