#include "ui/PackTab.h"

#include "core/Packer.h"
#include "core/ToolProcess.h"
#include "ui/GameCombo.h"

#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QFileDialog>
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QProcess>
#include <QPushButton>
#include <QSettings>
#include <QVBoxLayout>

namespace {

enum OutputMode { ParentFolder, WorkFolder };

}

PackTab::PackTab(GameList *games, QWidget *parent)
    : QWidget(parent)
    , m_process(new ToolProcess(this))
    , m_game(new GameCombo(games, "vpk", "pack/game", this))
    , m_format(new QComboBox(this))
    , m_form(new QFormLayout)
    , m_multiChunk(new QCheckBox(tr("Multi-file VPK (-M)"), this))
    , m_warnInvalid(new QCheckBox(tr("Skip invalid files (-warninvalid)"), this))
    , m_extraOptions(new QLineEdit(this))
    , m_gmaRow(new QWidget(this))
    , m_gmaTitle(new QLineEdit(this))
    , m_gmaType(new QComboBox(this))
    , m_gmaTag1(new QComboBox(this))
    , m_gmaTag2(new QComboBox(this))
    , m_folder(new QLineEdit(this))
    , m_outputMode(new QComboBox(this))
    , m_outputPath(new QLineEdit(this))
    , m_outputBrowse(new QPushButton(tr("Browse…"), this))
    , m_pack(new QPushButton(tr("Pack"), this))
    , m_cancel(new QPushButton(tr("Cancel"), this))
    , m_log(new QPlainTextEdit(this))
{
    const QSettings settings;
    m_format->addItem(tr("VPK (vpk)"), int(packer::Format::Vpk));
    m_format->addItem(tr("GMA (gmad)"), int(packer::Format::Gma));
    m_format->setCurrentIndex(qBound(0, settings.value("pack/format", 0).toInt(), 1));
    m_multiChunk->setChecked(settings.value("pack/multiChunk", false).toBool());
    m_warnInvalid->setChecked(settings.value("pack/warnInvalid", true).toBool());
    m_extraOptions->setText(settings.value("pack/options").toString());
    m_extraOptions->setPlaceholderText(tr("Additional packer options"));
    m_gmaTitle->setText(settings.value("pack/gmaTitle").toString());
    m_gmaTitle->setPlaceholderText(tr("Title (defaults to the folder name)"));
    m_gmaType->addItems(packer::addonTypes());
    m_gmaType->setCurrentText(settings.value("pack/gmaType", "model").toString());
    for (QComboBox *tag : {m_gmaTag1, m_gmaTag2}) {
        tag->addItem(tr("(no tag)"), QString());
        for (const QString &name : packer::addonTags())
            tag->addItem(name, name);
    }
    m_gmaTag1->setCurrentIndex(qMax(0, m_gmaTag1->findData(settings.value("pack/gmaTag1").toString())));
    m_gmaTag2->setCurrentIndex(qMax(0, m_gmaTag2->findData(settings.value("pack/gmaTag2").toString())));
    m_gmaRow->setToolTip(tr("Used to create addon.json when the folder does not have one yet."));
    m_folder->setText(settings.value("pack/folder").toString());
    m_folder->setPlaceholderText(tr("Folder to pack"));

    m_outputMode->addItem(tr("Parent of input folder"), ParentFolder);
    m_outputMode->addItem(tr("Work folder"), WorkFolder);
    m_outputMode->setCurrentIndex(qBound(0, settings.value("pack/outputMode", 0).toInt(), 1));
    m_outputPath->setText(settings.value("pack/outputPath").toString());
    m_outputPath->setPlaceholderText(tr("Folder for the packed file"));

    m_log->setReadOnly(true);
    m_log->setLineWrapMode(QPlainTextEdit::NoWrap);
    m_log->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));

    auto *browseButton = new QPushButton(tr("Browse…"), this);
    auto *folderRow = new QHBoxLayout;
    folderRow->addWidget(m_folder, 1);
    folderRow->addWidget(browseButton);

    auto *gameRow = new QHBoxLayout;
    gameRow->addWidget(m_game, 1);
    gameRow->addWidget(m_format);

    auto *outputRow = new QHBoxLayout;
    outputRow->addWidget(m_outputMode);
    outputRow->addWidget(m_outputPath, 1);
    outputRow->addWidget(m_outputBrowse);

    auto *gmaLayout = new QHBoxLayout(m_gmaRow);
    gmaLayout->setContentsMargins(0, 0, 0, 0);
    gmaLayout->addWidget(m_gmaTitle, 1);
    gmaLayout->addWidget(m_gmaType);
    gmaLayout->addWidget(m_gmaTag1);
    gmaLayout->addWidget(m_gmaTag2);

    auto *form = m_form;
    form->addRow(tr("Game:"), gameRow);
    form->addRow(tr("Folder:"), folderRow);
    form->addRow(tr("Output to:"), outputRow);
    auto *optionsRow = new QHBoxLayout;
    optionsRow->addWidget(m_multiChunk);
    optionsRow->addWidget(m_warnInvalid);
    optionsRow->addWidget(m_extraOptions, 1);
    form->addRow(tr("Options:"), optionsRow);
    form->addRow(tr("addon.json:"), m_gmaRow);

    auto *buttons = new QHBoxLayout;
    buttons->addStretch(1);
    buttons->addWidget(m_pack);
    buttons->addWidget(m_cancel);

    auto *layout = new QVBoxLayout(this);
    layout->addLayout(form);
    layout->addLayout(buttons);
    layout->addWidget(m_log, 1);

    connect(browseButton, &QPushButton::clicked, this, &PackTab::browse);
    connect(m_outputBrowse, &QPushButton::clicked, this, &PackTab::browseOutput);
    connect(m_outputMode, &QComboBox::currentIndexChanged, this, &PackTab::updateOutputWidgets);
    connect(m_format, &QComboBox::currentIndexChanged, this, &PackTab::updateFormatWidgets);
    connect(m_pack, &QPushButton::clicked, this, &PackTab::pack);
    connect(m_cancel, &QPushButton::clicked, m_process, &ToolProcess::cancel);

    connect(m_process, &ToolProcess::outputLine, m_log, &QPlainTextEdit::appendPlainText);
    connect(m_process, &ToolProcess::failedToStart, this, [this](const QString &error) {
        m_log->appendPlainText(tr("ERROR: %1").arg(error));
        setRunning(false);
    });
    connect(m_process, &ToolProcess::finished, this, [this](int exitCode, bool success) {
        QStringList files;
        if (success) {
            QStringList log;
            packer::Options options;
            options.multiChunk = m_pendingMultiChunk;
            files = packer::finishPack(packer::Format(m_format->currentData().toInt()), m_pendingFolder,
                                       m_pendingOutputFile, options, m_started, log);
            for (const QString &line : log)
                m_log->appendPlainText(line);
        }
        m_log->appendPlainText(files.isEmpty() ? tr("\nPack failed (exit code %1).").arg(exitCode)
                                               : tr("\nPacked:\n%1").arg(files.join(u'\n')));
        setRunning(false);
    });

    updateOutputWidgets();
    updateFormatWidgets();
    setRunning(false);
}

PackTab::~PackTab()
{
    saveSettings();
}

void PackTab::browse()
{
    const QString dir = QFileDialog::getExistingDirectory(this, tr("Select folder to pack"), m_folder->text());
    if (!dir.isEmpty())
        m_folder->setText(dir);
}

void PackTab::browseOutput()
{
    const QString dir = QFileDialog::getExistingDirectory(this, tr("Select output folder"), m_outputPath->text());
    if (!dir.isEmpty())
        m_outputPath->setText(dir);
}

void PackTab::updateOutputWidgets()
{
    const bool workFolder = m_outputMode->currentData().toInt() == WorkFolder;
    m_outputPath->setVisible(workFolder);
    m_outputBrowse->setVisible(workFolder);
}

void PackTab::updateFormatWidgets()
{
    const bool gma = packer::Format(m_format->currentData().toInt()) == packer::Format::Gma;
    m_multiChunk->setVisible(!gma);
    m_warnInvalid->setVisible(gma);
    m_form->setRowVisible(m_gmaRow, gma);
}

bool PackTab::prepareAddonJson(const QString &folder)
{
    if (packer::hasAddonJson(folder)) {
        m_log->appendPlainText(tr("Using the existing addon.json."));
        return true;
    }
    QString title = m_gmaTitle->text().trimmed();
    if (title.isEmpty())
        title = QFileInfo(folder).fileName();
    QStringList tags;
    for (const QComboBox *tag : {m_gmaTag1, m_gmaTag2}) {
        const QString name = tag->currentData().toString();
        if (!name.isEmpty() && !tags.contains(name))
            tags << name;
    }
    QString error;
    if (!packer::writeAddonJson(folder, title, m_gmaType->currentText(), tags, &error)) {
        m_log->appendPlainText(tr("ERROR: Could not create addon.json: %1").arg(error));
        return false;
    }
    m_log->appendPlainText(tr("Created addon.json (\"%1\", %2).").arg(title, m_gmaType->currentText()));
    return true;
}

void PackTab::pack()
{
    const GameCombo::Target *selected = m_game->target();
    if (!selected) {
        m_log->appendPlainText(tr("ERROR: No game with packing tools is selected."));
        return;
    }
    const GameCombo::Target target = *selected;
    saveSettings();
    m_log->clear();

    const QString folder = QDir::cleanPath(m_folder->text().trimmed());
    const QFileInfo folderInfo(folder);
    const bool workFolder = m_outputMode->currentData().toInt() == WorkFolder;
    const QString outputDir = workFolder ? m_outputPath->text().trimmed() : folderInfo.absolutePath();
    if (m_folder->text().trimmed().isEmpty() || outputDir.isEmpty()) {
        m_log->appendPlainText(tr("ERROR: Select a folder to pack and an output folder."));
        return;
    }

    const auto format = packer::Format(m_format->currentData().toInt());
    m_pendingOutputFile = outputDir + u'/' + folderInfo.fileName() + u'.' + packer::extension(format);
    m_pendingFolder = folder;
    m_pendingMultiChunk = format == packer::Format::Vpk && m_multiChunk->isChecked();
    m_started = QDateTime::currentDateTime();
    if (format == packer::Format::Gma && folderInfo.isDir() && !prepareAddonJson(folder))
        return;

    packer::Options options;
    options.multiChunk = m_pendingMultiChunk;
    options.warnInvalid = m_warnInvalid->isChecked();
    options.extra = QProcess::splitCommand(m_extraOptions->text());
    const launcher::Command command = packer::packCommand(target.app, format, folder, m_pendingOutputFile, options);
    if (command.isValid())
        m_log->appendPlainText(command.displayString() + '\n');
    setRunning(true);
    m_process->start(command);
}

void PackTab::setRunning(bool running)
{
    m_pack->setEnabled(!running);
    m_cancel->setEnabled(running);
    m_game->setEnabled(!running);
}

void PackTab::saveSettings() const
{
    m_game->save();
    QSettings settings;
    settings.setValue("pack/format", m_format->currentIndex());
    settings.setValue("pack/multiChunk", m_multiChunk->isChecked());
    settings.setValue("pack/warnInvalid", m_warnInvalid->isChecked());
    settings.setValue("pack/options", m_extraOptions->text());
    settings.setValue("pack/gmaTitle", m_gmaTitle->text());
    settings.setValue("pack/gmaType", m_gmaType->currentText());
    settings.setValue("pack/gmaTag1", m_gmaTag1->currentData());
    settings.setValue("pack/gmaTag2", m_gmaTag2->currentData());
    settings.setValue("pack/folder", m_folder->text());
    settings.setValue("pack/outputMode", m_outputMode->currentIndex());
    settings.setValue("pack/outputPath", m_outputPath->text());
}
