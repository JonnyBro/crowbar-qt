#include "ui/CompileTab.h"

#include "core/Studiomdl.h"
#include "core/ToolProcess.h"
#include "ui/GameList.h"

#include <QCheckBox>
#include <QComboBox>
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

enum OutputMode { GameModels, QcSubfolder, WorkFolder };

QString targetKey(const steam::App &app, const QString &gameDir)
{
    return QStringLiteral("%1:%2").arg(app.id).arg(QFileInfo(gameDir).fileName());
}

} // namespace

CompileTab::CompileTab(GameList *games, QWidget *parent)
    : QWidget(parent)
    , m_games(games)
    , m_process(new ToolProcess(this))
    , m_game(new QComboBox(this))
    , m_qcFile(new QLineEdit(this))
    , m_outputMode(new QComboBox(this))
    , m_outputSubfolder(new QLineEdit(this))
    , m_outputPath(new QLineEdit(this))
    , m_outputBrowse(new QPushButton(tr("Browse…"), this))
    , m_noP4(new QCheckBox(tr("No P4 (-nop4)"), this))
    , m_verbose(new QCheckBox(tr("Verbose (-verbose)"), this))
    , m_extraOptions(new QLineEdit(this))
    , m_compile(new QPushButton(tr("Compile"), this))
    , m_cancel(new QPushButton(tr("Cancel"), this))
    , m_log(new QPlainTextEdit(this))
{
    const QSettings settings;
    m_qcFile->setText(settings.value("compile/qc").toString());
    m_qcFile->setPlaceholderText(tr("Path to a .qc file"));
    m_outputMode->addItem(tr("Game's \"models\" folder"), GameModels);
    m_outputMode->addItem(tr("Subfolder (of QC input)"), QcSubfolder);
    m_outputMode->addItem(tr("Work folder"), WorkFolder);
    m_outputMode->setCurrentIndex(qBound(0, settings.value("compile/outputMode", 0).toInt(), 2));
    m_outputSubfolder->setText(settings.value("compile/outputSubfolder", "compiled").toString());
    m_outputPath->setText(settings.value("compile/outputPath").toString());
    m_outputPath->setPlaceholderText(tr("Folder for compiled files"));
    m_noP4->setChecked(settings.value("compile/nop4", true).toBool());
    m_verbose->setChecked(settings.value("compile/verbose", false).toBool());
    m_extraOptions->setText(settings.value("compile/options").toString());
    m_extraOptions->setPlaceholderText(tr("Additional studiomdl options"));

    m_log->setReadOnly(true);
    m_log->setLineWrapMode(QPlainTextEdit::NoWrap);
    m_log->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));

    auto *browseButton = new QPushButton(tr("Browse…"), this);
    auto *qcRow = new QHBoxLayout;
    qcRow->addWidget(m_qcFile, 1);
    qcRow->addWidget(browseButton);

    auto *outputRow = new QHBoxLayout;
    outputRow->addWidget(m_outputMode);
    outputRow->addWidget(m_outputSubfolder, 1);
    outputRow->addWidget(m_outputPath, 1);
    outputRow->addWidget(m_outputBrowse);

    auto *optionsRow = new QHBoxLayout;
    optionsRow->addWidget(m_noP4);
    optionsRow->addWidget(m_verbose);
    optionsRow->addWidget(m_extraOptions, 1);

    auto *form = new QFormLayout;
    form->addRow(tr("Game:"), m_game);
    form->addRow(tr("QC file:"), qcRow);
    form->addRow(tr("Output to:"), outputRow);
    form->addRow(tr("Options:"), optionsRow);

    auto *buttons = new QHBoxLayout;
    buttons->addStretch(1);
    buttons->addWidget(m_compile);
    buttons->addWidget(m_cancel);

    auto *layout = new QVBoxLayout(this);
    layout->addLayout(form);
    layout->addLayout(buttons);
    layout->addWidget(m_log, 1);

    connect(browseButton, &QPushButton::clicked, this, &CompileTab::browse);
    connect(m_outputBrowse, &QPushButton::clicked, this, &CompileTab::browseOutput);
    connect(m_outputMode, &QComboBox::currentIndexChanged, this, &CompileTab::updateOutputWidgets);
    connect(m_compile, &QPushButton::clicked, this, &CompileTab::compile);
    connect(m_cancel, &QPushButton::clicked, m_process, &ToolProcess::cancel);
    connect(m_games, &GameList::changed, this, &CompileTab::reloadGames);

    connect(m_process, &ToolProcess::outputLine, m_log, &QPlainTextEdit::appendPlainText);
    connect(m_process, &ToolProcess::failedToStart, this, [this](const QString &error) {
        m_log->appendPlainText(tr("ERROR: %1").arg(error));
        setRunning(false);
    });
    connect(m_process, &ToolProcess::finished, this, [this](int exitCode, bool success) {
        if (success)
            moveOutput();
        m_log->appendPlainText(success ? tr("\nCompile finished.")
                                       : tr("\nCompile failed (exit code %1).").arg(exitCode));
        setRunning(false);
    });

    updateOutputWidgets();
    setRunning(false);
    reloadGames();
}

CompileTab::~CompileTab()
{
    saveSettings();
}

void CompileTab::reloadGames()
{
    const QString previous = m_game->count() > 0
        ? m_game->currentData().toString()
        : QSettings().value("compile/game").toString();

    m_targets.clear();
    m_game->clear();
    for (const steam::App &app : m_games->apps()) {
        if (studiomdl::findExecutable(app).isEmpty())
            continue;
        for (const QString &gameDir : app.gameDirs) {
            m_targets << Target{app, gameDir};
            m_game->addItem(QStringLiteral("%1 — %2").arg(app.name, QFileInfo(gameDir).fileName()),
                            targetKey(app, gameDir));
        }
    }

    const int index = m_game->findData(previous);
    if (index >= 0)
        m_game->setCurrentIndex(index);
    setRunning(m_process->isRunning());
}

void CompileTab::browse()
{
    const QString file = QFileDialog::getOpenFileName(
        this, tr("Select QC file"), QFileInfo(m_qcFile->text()).absolutePath(), tr("QC files (*.qc);;All files (*)"));
    if (!file.isEmpty())
        m_qcFile->setText(file);
}

void CompileTab::browseOutput()
{
    const QString dir = QFileDialog::getExistingDirectory(this, tr("Select output folder"), m_outputPath->text());
    if (!dir.isEmpty())
        m_outputPath->setText(dir);
}

void CompileTab::updateOutputWidgets()
{
    const int mode = m_outputMode->currentData().toInt();
    m_outputSubfolder->setVisible(mode == QcSubfolder);
    m_outputPath->setVisible(mode == WorkFolder);
    m_outputBrowse->setVisible(mode == WorkFolder);
}

QString CompileTab::outputDir() const
{
    switch (m_outputMode->currentData().toInt()) {
    case QcSubfolder:
        return QFileInfo(m_qcFile->text().trimmed()).absolutePath() + '/' + m_outputSubfolder->text().trimmed();
    case WorkFolder:
        return m_outputPath->text().trimmed();
    default:
        return {};
    }
}

void CompileTab::moveOutput()
{
    if (m_pendingOutputDir.isEmpty())
        return;
    m_log->appendPlainText({});
    if (m_pendingModelName.isEmpty()) {
        m_log->appendPlainText(tr("WARNING: No $modelname found in the QC file; compiled files were left in the game's \"models\" folder."));
        return;
    }
    QStringList log;
    studiomdl::moveCompiledModel(m_pendingGameDir, m_pendingModelName, m_pendingOutputDir, log);
    for (const QString &line : log)
        m_log->appendPlainText(line);
}

void CompileTab::compile()
{
    const int index = m_game->currentIndex();
    if (index < 0 || index >= m_targets.size())
        return;
    saveSettings();

    const Target &target = m_targets[index];
    studiomdl::Options options;
    options.noP4 = m_noP4->isChecked();
    options.verbose = m_verbose->isChecked();
    options.extra = QProcess::splitCommand(m_extraOptions->text());

    const launcher::Command command =
        studiomdl::compileCommand(target.app, target.gameDir, m_qcFile->text().trimmed(), options);

    m_log->clear();
    if (m_outputMode->currentData().toInt() == WorkFolder && outputDir().isEmpty()) {
        m_log->appendPlainText(tr("ERROR: Select an output folder."));
        return;
    }
    m_pendingGameDir = target.gameDir;
    m_pendingOutputDir = outputDir();
    m_pendingModelName = studiomdl::readModelName(m_qcFile->text().trimmed());
    if (command.isValid())
        m_log->appendPlainText(command.displayString() + '\n');
    setRunning(true);
    m_process->start(command);
}

void CompileTab::setRunning(bool running)
{
    m_compile->setEnabled(!running && m_game->count() > 0);
    m_cancel->setEnabled(running);
    m_game->setEnabled(!running);
}

void CompileTab::saveSettings() const
{
    QSettings settings;
    settings.setValue("compile/game", m_game->currentData());
    settings.setValue("compile/qc", m_qcFile->text());
    settings.setValue("compile/outputMode", m_outputMode->currentIndex());
    settings.setValue("compile/outputSubfolder", m_outputSubfolder->text());
    settings.setValue("compile/outputPath", m_outputPath->text());
    settings.setValue("compile/nop4", m_noP4->isChecked());
    settings.setValue("compile/verbose", m_verbose->isChecked());
    settings.setValue("compile/options", m_extraOptions->text());
}
