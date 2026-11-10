#include "ui/ViewTab.h"

#include "core/Hlmv.h"
#include "core/ToolProcess.h"
#include "ui/GameCombo.h"

#include <QFileDialog>
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSettings>
#include <QVBoxLayout>

ViewTab::ViewTab(GameList *games, QWidget *parent)
    : QWidget(parent)
    , m_game(new GameCombo(games, "hlmv", "view/game", this))
    , m_mdlFile(new QLineEdit(this))
    , m_view(new QPushButton(tr("View"), this))
    , m_openViewer(new QPushButton(tr("Open Viewer"), this))
    , m_log(new QPlainTextEdit(this))
{
    m_mdlFile->setText(QSettings().value("view/mdl").toString());
    m_mdlFile->setPlaceholderText(tr("Path to a .mdl file"));

    m_log->setReadOnly(true);
    m_log->setLineWrapMode(QPlainTextEdit::NoWrap);
    m_log->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));

    auto *browseButton = new QPushButton(tr("Browse…"), this);
    auto *mdlRow = new QHBoxLayout;
    mdlRow->addWidget(m_mdlFile, 1);
    mdlRow->addWidget(browseButton);

    auto *form = new QFormLayout;
    form->addRow(tr("Game:"), m_game);
    form->addRow(tr("MDL file:"), mdlRow);

    auto *buttons = new QHBoxLayout;
    buttons->addStretch(1);
    buttons->addWidget(m_view);
    buttons->addWidget(m_openViewer);

    auto *layout = new QVBoxLayout(this);
    layout->addLayout(form);
    layout->addLayout(buttons);
    layout->addWidget(m_log, 1);

    connect(browseButton, &QPushButton::clicked, this, &ViewTab::browse);
    connect(m_view, &QPushButton::clicked, this, [this] { launch(true); });
    connect(m_openViewer, &QPushButton::clicked, this, [this] { launch(false); });
}

ViewTab::~ViewTab()
{
    saveSettings();
}

void ViewTab::browse()
{
    const QString file = QFileDialog::getOpenFileName(
        this, tr("Select MDL file"), QFileInfo(m_mdlFile->text()).absolutePath(), tr("MDL files (*.mdl);;All files (*)"));
    if (!file.isEmpty())
        m_mdlFile->setText(file);
}

void ViewTab::launch(bool withModel)
{
    const GameCombo::Target *target = m_game->target();
    if (!target)
        return;
    saveSettings();

    const QString mdlFile = withModel ? m_mdlFile->text().trimmed() : QString();
    if (withModel && mdlFile.isEmpty()) {
        m_log->appendPlainText(tr("ERROR: Select an MDL file."));
        return;
    }

    const launcher::Command command = hlmv::viewCommand(target->app, target->gameDir, mdlFile);
    if (command.isValid())
        m_log->appendPlainText(command.displayString());

    // One process per viewer window, so several models can be open at once.
    auto *process = new ToolProcess(this);
    connect(process, &ToolProcess::outputLine, m_log, &QPlainTextEdit::appendPlainText);
    connect(process, &ToolProcess::failedToStart, this, [this, process](const QString &error) {
        m_log->appendPlainText(tr("ERROR: %1").arg(error));
        process->deleteLater();
    });
    connect(process, &ToolProcess::finished, this, [this, process] {
        m_log->appendPlainText(tr("Viewer closed."));
        process->deleteLater();
    });
    process->start(command);
}

void ViewTab::saveSettings() const
{
    m_game->save();
    QSettings().setValue("view/mdl", m_mdlFile->text());
}
