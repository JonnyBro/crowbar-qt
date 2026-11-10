#include "ui/PublishTab.h"

#include "core/Packer.h"
#include "core/ToolProcess.h"
#include "ui/GameCombo.h"

#include <QDateTime>
#include <QDesktopServices>
#include <QFileDialog>
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QMenu>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRegularExpressionValidator>
#include <QSettings>
#include <QSplitter>
#include <QTreeWidget>
#include <QUrl>
#include <QVBoxLayout>

namespace {

enum Column { Title, Id, Size };

}

PublishTab::PublishTab(GameList *games, QWidget *parent)
    : QWidget(parent)
    , m_process(new ToolProcess(this))
    , m_game(new GameCombo(games, "gmpublish", "publish/game", this))
    , m_items(new QTreeWidget(this))
    , m_refresh(new QPushButton(tr("Refresh My Items"), this))
    , m_itemId(new QLineEdit(this))
    , m_itemHint(new QLabel(this))
    , m_content(new QLineEdit(this))
    , m_icon(new QLineEdit(this))
    , m_changes(new QLineEdit(this))
    , m_publish(new QPushButton(tr("Publish"), this))
    , m_cancel(new QPushButton(tr("Cancel"), this))
    , m_log(new QPlainTextEdit(this))
{
    const QSettings settings;
    m_content->setText(settings.value("publish/content").toString());
    m_content->setPlaceholderText(tr("A .gma file, or a folder with addon.json to pack first"));
    m_icon->setText(settings.value("publish/icon").toString());
    m_icon->setPlaceholderText(tr("Any image; converted to a 512x512 JPEG if needed"));
    m_changes->setPlaceholderText(tr("Change notes (updates only)"));
    m_itemId->setPlaceholderText(tr("Empty creates a new item"));
    m_itemId->setValidator(new QRegularExpressionValidator(QRegularExpression("\\d{0,20}"), m_itemId));

    m_items->setHeaderLabels({tr("Title"), tr("ID"), tr("Size")});
    m_items->setRootIsDecorated(false);
    m_items->setAlternatingRowColors(true);
    m_items->setUniformRowHeights(true);
    m_items->header()->setStretchLastSection(false);
    m_items->header()->setSectionResizeMode(Title, QHeaderView::Stretch);
    m_items->header()->setSectionResizeMode(Id, QHeaderView::ResizeToContents);
    m_items->header()->setSectionResizeMode(Size, QHeaderView::ResizeToContents);
    m_items->setToolTip(tr("Select an item to update it. Double-click opens its workshop page."));

    m_log->setReadOnly(true);
    m_log->setLineWrapMode(QPlainTextEdit::NoWrap);
    m_log->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));

    auto *newItem = new QPushButton(tr("New Item"), this);
    auto *idRow = new QHBoxLayout;
    idRow->addWidget(m_itemId, 1);
    idRow->addWidget(m_itemHint, 2);
    idRow->addWidget(newItem);

    auto *contentBrowse = new QPushButton(tr("Browse…"), this);
    auto *contentRow = new QHBoxLayout;
    contentRow->addWidget(m_content, 1);
    contentRow->addWidget(contentBrowse);

    auto *iconBrowse = new QPushButton(tr("Browse…"), this);
    auto *iconRow = new QHBoxLayout;
    iconRow->addWidget(m_icon, 1);
    iconRow->addWidget(iconBrowse);

    auto *gameRow = new QHBoxLayout;
    gameRow->addWidget(m_game, 1);
    gameRow->addWidget(m_refresh);

    auto *form = new QFormLayout;
    form->addRow(tr("Game:"), gameRow);
    form->addRow(tr("Item ID:"), idRow);
    form->addRow(tr("Content:"), contentRow);
    form->addRow(tr("Icon:"), iconRow);
    form->addRow(tr("Changes:"), m_changes);

    auto *buttons = new QHBoxLayout;
    buttons->addStretch(1);
    buttons->addWidget(m_publish);
    buttons->addWidget(m_cancel);

    auto *splitter = new QSplitter(Qt::Vertical, this);
    splitter->addWidget(m_items);
    splitter->addWidget(m_log);
    splitter->setStretchFactor(0, 2);
    splitter->setStretchFactor(1, 1);

    auto *layout = new QVBoxLayout(this);
    layout->addLayout(form);
    layout->addLayout(buttons);
    layout->addWidget(splitter, 1);

    connect(m_refresh, &QPushButton::clicked, this, &PublishTab::refreshList);
    connect(newItem, &QPushButton::clicked, this, [this] {
        m_items->clearSelection();
        m_itemId->clear();
    });
    connect(contentBrowse, &QPushButton::clicked, this, &PublishTab::browseContent);
    connect(iconBrowse, &QPushButton::clicked, this, &PublishTab::browseIcon);
    connect(m_publish, &QPushButton::clicked, this, &PublishTab::publish);
    connect(m_cancel, &QPushButton::clicked, m_process, &ToolProcess::cancel);
    connect(m_itemId, &QLineEdit::textChanged, this, [this](const QString &text) {
        m_itemHint->setText(text.isEmpty() ? tr("New item") : tr("Update existing item"));
        m_changes->setEnabled(!text.isEmpty());
        m_publish->setText(text.isEmpty() ? tr("Publish New Item") : tr("Update Item"));
    });
    connect(m_items, &QTreeWidget::itemSelectionChanged, this, [this] {
        const QList<QTreeWidgetItem *> selected = m_items->selectedItems();
        if (!selected.isEmpty())
            m_itemId->setText(selected.first()->text(Id));
    });
    connect(m_items, &QTreeWidget::itemActivated, this, [](QTreeWidgetItem *item) {
        QDesktopServices::openUrl(QUrl(publisher::workshopUrl(item->text(Id).toULongLong())));
    });

    connect(m_process, &ToolProcess::outputLine, this, &PublishTab::onLine);
    connect(m_process, &ToolProcess::failedToStart, this, [this](const QString &error) {
        m_log->appendPlainText(tr("ERROR: %1").arg(error));
        setStage(Stage::Idle);
    });
    connect(m_process, &ToolProcess::finished, this, &PublishTab::onFinished);

    m_itemId->setText(settings.value("publish/id").toString());
    emit m_itemId->textChanged(m_itemId->text());
    setStage(Stage::Idle);
}

PublishTab::~PublishTab()
{
    saveSettings();
}

void PublishTab::refreshList()
{
    const GameCombo::Target *target = m_game->target();
    if (!target || m_stage != Stage::Idle)
        return;
    m_parser = {};
    m_items->clear();
    m_log->appendPlainText(tr("Getting your workshop items…"));
    setStage(Stage::Listing);
    m_process->start(publisher::listCommand(target->app));
}

void PublishTab::browseContent()
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

void PublishTab::browseIcon()
{
    const QString file = QFileDialog::getOpenFileName(
        this, tr("Select icon"), QFileInfo(m_icon->text()).absolutePath(),
        tr("Images (*.jpg *.jpeg *.png *.bmp *.gif *.webp);;All files (*)"));
    if (!file.isEmpty())
        m_icon->setText(file);
}

void PublishTab::publish()
{
    const GameCombo::Target *target = m_game->target();
    if (!target || m_stage != Stage::Idle)
        return;
    saveSettings();
    m_log->clear();

    m_request = {};
    m_request.id = m_itemId->text().toULongLong();
    m_request.changes = m_changes->text();
    m_createdId = 0;

    const QString icon = m_icon->text().trimmed();
    if (!icon.isEmpty()) {
        QString error;
        m_request.iconFile = publisher::prepareIcon(icon, m_tempDir.filePath("icon.jpg"), &error);
        if (m_request.iconFile.isEmpty()) {
            m_log->appendPlainText(tr("ERROR: %1").arg(error));
            return;
        }
        if (m_request.iconFile != icon)
            m_log->appendPlainText(tr("Icon converted to a 512x512 JPEG."));
    }

    const QString content = m_content->text().trimmed();
    const bool folder = QFileInfo(content).isDir();
    const QString what = m_request.id == 0
        ? tr("Create a new workshop item from \"%1\"?").arg(QFileInfo(content).fileName())
        : tr("Upload to workshop item %1?").arg(m_request.id);
    const QString note = m_request.id == 0 ? tr("\n\nNew items start hidden; make them public on the workshop page.")
                                           : QString();
    if (QMessageBox::question(this, tr("Publish"), what + note) != QMessageBox::Yes)
        return;

    if (!folder) {
        startUpload(content);
        return;
    }

    // A folder is packed with gmad first, into a temporary GMA.
    m_packedFile = m_tempDir.filePath(QFileInfo(content).fileName() + ".gma");
    QFile::remove(m_packedFile);
    packer::Options options;
    options.warnInvalid = true;
    const launcher::Command command = packer::packCommand(target->app, packer::Format::Gma, content, m_packedFile, options);
    if (!command.isValid()) {
        m_log->appendPlainText(tr("ERROR: %1").arg(command.error));
        return;
    }
    m_log->appendPlainText(command.displayString() + '\n');
    setStage(Stage::Packing);
    m_process->start(command);
}

void PublishTab::startUpload(const QString &gmaFile)
{
    const GameCombo::Target *target = m_game->target();
    if (!target) {
        setStage(Stage::Idle);
        return;
    }
    m_request.gmaFile = gmaFile;
    const launcher::Command command = publisher::publishCommand(target->app, m_request);
    if (!command.isValid()) {
        m_log->appendPlainText(tr("ERROR: %1").arg(command.error));
        setStage(Stage::Idle);
        return;
    }
    m_log->appendPlainText(command.displayString() + '\n');
    setStage(Stage::Publishing);
    m_process->start(command);
}

void PublishTab::onLine(const QString &line)
{
    if (m_stage == Stage::Listing) {
        m_parser.addLine(line);
        return;
    }
    if (const quint64 id = publisher::parseCreatedId(line))
        m_createdId = id;
    m_log->appendPlainText(line);
}

void PublishTab::onFinished(int exitCode, bool success)
{
    switch (m_stage) {
    case Stage::Listing: {
        for (const publisher::WorkshopItem &item : m_parser.items()) {
            auto *row = new QTreeWidgetItem(m_items);
            row->setText(Title, item.title);
            row->setText(Id, QString::number(item.id));
            row->setText(Size, item.size);
            row->setTextAlignment(Size, Qt::AlignRight | Qt::AlignVCenter);
            if (QString::number(item.id) == m_itemId->text())
                row->setSelected(true);
        }
        m_log->appendPlainText(success ? tr("%n item(s).", nullptr, int(m_parser.items().size()))
                                       : tr("Could not get the item list (exit code %1). Is Steam running?").arg(exitCode));
        setStage(Stage::Idle);
        break;
    }
    case Stage::Packing:
        if (!success || !QFile::exists(m_packedFile)) {
            m_log->appendPlainText(tr("\nPacking failed (exit code %1).").arg(exitCode));
            setStage(Stage::Idle);
            break;
        }
        m_log->appendPlainText({});
        startUpload(m_packedFile);
        break;
    case Stage::Publishing:
        if (m_createdId) {
            m_itemId->setText(QString::number(m_createdId));
            m_log->appendPlainText(tr("\nCreated item %1: %2").arg(m_createdId).arg(publisher::workshopUrl(m_createdId)));
        } else {
            m_log->appendPlainText(success ? tr("\ngmpublish finished; check the log above for errors.")
                                           : tr("\nPublish failed (exit code %1).").arg(exitCode));
        }
        setStage(Stage::Idle);
        if (success)
            refreshList();
        break;
    case Stage::Idle:
        break;
    }
}

void PublishTab::setStage(Stage stage)
{
    m_stage = stage;
    const bool idle = stage == Stage::Idle;
    m_publish->setEnabled(idle);
    m_refresh->setEnabled(idle);
    m_game->setEnabled(idle);
    m_cancel->setEnabled(!idle);
}

void PublishTab::saveSettings() const
{
    m_game->save();
    QSettings settings;
    settings.setValue("publish/id", m_itemId->text());
    settings.setValue("publish/content", m_content->text());
    settings.setValue("publish/icon", m_icon->text());
}
