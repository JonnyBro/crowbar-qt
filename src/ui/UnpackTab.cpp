#include "ui/UnpackTab.h"

#include <QComboBox>
#include <QDesktopServices>
#include <QFileDialog>
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHash>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QLocale>
#include <QPlainTextEdit>
#include <QProgressBar>
#include <QPushButton>
#include <QSettings>
#include <QSplitter>
#include <QTreeWidget>
#include <QUrl>
#include <QSet>
#include <QVBoxLayout>

#include <algorithm>

namespace {

enum OutputMode { PackageSubfolder, WorkFolder };
enum { EntryIndexRole = Qt::UserRole };

void collectEntries(const QTreeWidgetItem *item, QSet<int> &indexes)
{
    const int index = item->data(0, EntryIndexRole).toInt();
    if (index >= 0)
        indexes.insert(index);
    for (int i = 0; i < item->childCount(); ++i)
        collectEntries(item->child(i), indexes);
}

} // namespace

UnpackTab::UnpackTab(QWidget *parent)
    : QWidget(parent)
    , m_file(new QLineEdit(this))
    , m_info(new QLabel(this))
    , m_tree(new QTreeWidget(this))
    , m_outputMode(new QComboBox(this))
    , m_outputPath(new QLineEdit(this))
    , m_outputBrowse(new QPushButton(tr("Browse…"), this))
    , m_unpackAll(new QPushButton(tr("Unpack All"), this))
    , m_unpackSelected(new QPushButton(tr("Unpack Selected"), this))
    , m_cancel(new QPushButton(tr("Cancel"), this))
    , m_progress(new QProgressBar(this))
    , m_log(new QPlainTextEdit(this))
{
    const QSettings settings;
    m_file->setText(settings.value("unpack/file").toString());
    m_file->setPlaceholderText(tr("Path to a .vpk, .gma or workshop .bin file"));

    m_tree->setHeaderLabels({tr("Name"), tr("Size")});
    m_tree->setSelectionMode(QAbstractItemView::ExtendedSelection);
    m_tree->setUniformRowHeights(true);
    m_tree->setAlternatingRowColors(true);
    m_tree->header()->setStretchLastSection(false);
    m_tree->header()->setSectionResizeMode(0, QHeaderView::Stretch);
    // Fixed width: sizing to contents only measures rows that are expanded at that moment.
    m_tree->header()->setSectionResizeMode(1, QHeaderView::Fixed);
    m_tree->header()->resizeSection(1, m_tree->fontMetrics().horizontalAdvance("0000,00 MiB") + 32);
    m_tree->setToolTip(tr("Double-click a file to open it from a temporary copy."));

    m_outputMode->addItem(tr("Subfolder (of package)"), PackageSubfolder);
    m_outputMode->addItem(tr("Work folder"), WorkFolder);
    m_outputMode->setCurrentIndex(qBound(0, settings.value("unpack/outputMode", 0).toInt(), 1));
    m_outputPath->setText(settings.value("unpack/outputPath").toString());
    m_outputPath->setPlaceholderText(tr("Folder for unpacked files"));

    m_log->setReadOnly(true);
    m_log->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));

    auto *browseButton = new QPushButton(tr("Browse…"), this);
    auto *fileRow = new QHBoxLayout;
    fileRow->addWidget(m_file, 1);
    fileRow->addWidget(browseButton);

    auto *outputRow = new QHBoxLayout;
    outputRow->addWidget(m_outputMode);
    outputRow->addWidget(m_outputPath, 1);
    outputRow->addWidget(m_outputBrowse);

    auto *form = new QFormLayout;
    form->addRow(tr("Package:"), fileRow);
    form->addRow(tr("Output to:"), outputRow);

    auto *buttons = new QHBoxLayout;
    buttons->addWidget(m_info, 1);
    buttons->addWidget(m_unpackAll);
    buttons->addWidget(m_unpackSelected);
    buttons->addWidget(m_cancel);

    auto *splitter = new QSplitter(Qt::Vertical, this);
    splitter->addWidget(m_tree);
    splitter->addWidget(m_log);
    splitter->setStretchFactor(0, 3);
    splitter->setStretchFactor(1, 1);

    auto *layout = new QVBoxLayout(this);
    layout->addLayout(form);
    layout->addLayout(buttons);
    layout->addWidget(m_progress);
    layout->addWidget(splitter, 1);

    connect(browseButton, &QPushButton::clicked, this, &UnpackTab::browse);
    connect(m_file, &QLineEdit::editingFinished, this, &UnpackTab::loadPackage);
    connect(m_tree, &QTreeWidget::itemActivated, this, &UnpackTab::openItem);
    connect(m_outputBrowse, &QPushButton::clicked, this, &UnpackTab::browseOutput);
    connect(m_outputMode, &QComboBox::currentIndexChanged, this, &UnpackTab::updateOutputWidgets);
    connect(m_unpackAll, &QPushButton::clicked, this, [this] { unpack(false); });
    connect(m_unpackSelected, &QPushButton::clicked, this, [this] { unpack(true); });
    connect(m_cancel, &QPushButton::clicked, this, [this] {
        if (m_job)
            m_job->requestInterruption();
    });

    updateOutputWidgets();
    loadPackage();
}

UnpackTab::~UnpackTab()
{
    saveSettings();
    if (m_job) {
        m_job->disconnect(this);
        m_job->requestInterruption();
        m_job->wait();
    }
}

void UnpackTab::browse()
{
    const QString file = QFileDialog::getOpenFileName(
        this, tr("Select package"), QFileInfo(m_file->text()).absolutePath(),
        tr("Packages (*.vpk *.gma *.bin);;All files (*)"));
    if (!file.isEmpty()) {
        m_file->setText(file);
        loadPackage();
    }
}

void UnpackTab::browseOutput()
{
    const QString dir = QFileDialog::getExistingDirectory(this, tr("Select output folder"), m_outputPath->text());
    if (!dir.isEmpty())
        m_outputPath->setText(dir);
}

void UnpackTab::loadPackage()
{
    const QString file = m_file->text().trimmed();
    if (m_package && m_package->file() == file)
        return;

    m_tree->clear();
    m_package.reset();
    m_info->clear();
    setRunning(false);
    if (file.isEmpty())
        return;

    QString error;
    m_package = package::Package::open(file, &error);
    if (!m_package) {
        m_info->setText(tr("ERROR: %1").arg(error));
        setRunning(false);
        return;
    }

    const QLocale locale;
    QHash<QString, QTreeWidgetItem *> folders;
    const auto folderItem = [&](const QString &path, auto &&self) -> QTreeWidgetItem * {
        if (path.isEmpty())
            return m_tree->invisibleRootItem();
        if (QTreeWidgetItem *existing = folders.value(path))
            return existing;
        const qsizetype slash = path.lastIndexOf(u'/');
        auto *item = new QTreeWidgetItem(self(slash < 0 ? QString() : path.left(slash), self));
        item->setText(0, path.mid(slash + 1));
        item->setData(0, EntryIndexRole, -1);
        folders.insert(path, item);
        return item;
    };

    quint64 totalSize = 0;
    m_tree->setUpdatesEnabled(false);
    const QList<package::Entry> &entries = m_package->entries();
    for (int i = 0; i < entries.size(); ++i) {
        const package::Entry &entry = entries[i];
        const qsizetype slash = entry.path.lastIndexOf(u'/');
        auto *item = new QTreeWidgetItem(folderItem(slash < 0 ? QString() : entry.path.left(slash), folderItem));
        item->setText(0, entry.path.mid(slash + 1));
        item->setText(1, locale.formattedDataSize(qint64(entry.size)));
        item->setTextAlignment(1, Qt::AlignRight | Qt::AlignVCenter);
        item->setData(0, EntryIndexRole, i);
        totalSize += entry.size;
    }
    m_tree->sortItems(0, Qt::AscendingOrder);
    m_tree->setUpdatesEnabled(true);

    QString info = tr("%1, %n file(s), %2", nullptr, int(entries.size()))
                       .arg(m_package->format(), locale.formattedDataSize(qint64(totalSize)));
    if (!m_package->title().isEmpty())
        info = m_package->title() + " — " + info;
    m_info->setText(info);
    setRunning(false);
}

void UnpackTab::updateOutputWidgets()
{
    const bool workFolder = m_outputMode->currentData().toInt() == WorkFolder;
    m_outputPath->setVisible(workFolder);
    m_outputBrowse->setVisible(workFolder);
}

QString UnpackTab::outputDir() const
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

void UnpackTab::unpack(bool selectedOnly)
{
    loadPackage();
    if (!m_package || m_job)
        return;
    saveSettings();

    const QString output = outputDir();
    if (output.isEmpty()) {
        m_log->appendPlainText(tr("ERROR: Select an output folder."));
        return;
    }

    QList<int> indexes;
    if (selectedOnly) {
        QSet<int> selected;
        for (const QTreeWidgetItem *item : m_tree->selectedItems())
            collectEntries(item, selected);
        indexes = QList<int>(selected.begin(), selected.end());
        std::sort(indexes.begin(), indexes.end());
        if (indexes.isEmpty()) {
            m_log->appendPlainText(tr("ERROR: Nothing is selected."));
            return;
        }
    } else {
        indexes.reserve(m_package->entries().size());
        for (int i = 0; i < m_package->entries().size(); ++i)
            indexes << i;
    }

    m_log->appendPlainText(tr("Unpacking %n file(s) to \"%1\"…", nullptr, int(indexes.size())).arg(output));
    m_progress->setRange(0, int(indexes.size()));
    m_progress->setValue(0);

    m_job = new package::ExtractJob(*m_package, indexes, output, this);
    connect(m_job, &package::ExtractJob::progress, this, [this](int done, int) { m_progress->setValue(done); });
    connect(m_job, &package::ExtractJob::message, m_log, &QPlainTextEdit::appendPlainText);
    connect(m_job, &package::ExtractJob::done, this, [this](int extracted, int failed, bool cancelled) {
        m_log->appendPlainText(cancelled ? tr("Cancelled after %1 file(s).").arg(extracted)
                                         : tr("Unpacked %1 file(s), %2 failed.").arg(extracted).arg(failed));
        m_job->deleteLater();
        m_job = nullptr;
        setRunning(false);
    });
    setRunning(true);
    m_job->start();
}

void UnpackTab::openItem(QTreeWidgetItem *item)
{
    const int index = item->data(0, EntryIndexRole).toInt();
    if (!m_package || index < 0 || index >= m_package->entries().size())
        return;

    // Removed together with the tab, like Crowbar's temporary preview copies.
    const QString dir = m_tempDir.path() + u'/' + QFileInfo(m_package->file()).completeBaseName();
    const package::Entry &entry = m_package->entries().at(index);
    QString error;
    if (!m_tempDir.isValid() || !m_package->extract(entry, dir, &error)) {
        m_log->appendPlainText(tr("ERROR: Could not open \"%1\": %2").arg(entry.path, error));
        return;
    }
    const QString file = dir + u'/' + entry.path;
    if (!QDesktopServices::openUrl(QUrl::fromLocalFile(file)))
        m_log->appendPlainText(tr("No application is associated with \"%1\". Temporary copy: %2").arg(entry.path, file));
}

void UnpackTab::setRunning(bool running)
{
    const bool ready = !running && m_package.has_value();
    m_unpackAll->setEnabled(ready);
    m_unpackSelected->setEnabled(ready);
    m_cancel->setEnabled(running);
    m_file->setEnabled(!running);
}

void UnpackTab::saveSettings() const
{
    QSettings settings;
    settings.setValue("unpack/file", m_file->text());
    settings.setValue("unpack/outputMode", m_outputMode->currentIndex());
    settings.setValue("unpack/outputPath", m_outputPath->text());
}
