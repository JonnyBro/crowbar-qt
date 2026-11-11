#include "ui/CompilePage.h"

#include "core/Hlmv.h"
#include "core/Studiomdl.h"
#include "ui/GameList.h"
#include "ui/PathField.h"
#include "ui/Tasks.h"

#include <QCheckBox>
#include <QComboBox>
#include <QDirIterator>
#include <QDropEvent>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLineEdit>
#include <QMimeData>
#include <QProcess>
#include <QPushButton>
#include <QSettings>
#include <QStyle>
#include <QTreeWidget>
#include <QVBoxLayout>

namespace
{

	enum OutputMode
	{
		GameModels,
		QcSubfolder,
		WorkFolder
	};

	enum Column
	{
		FileColumn,
		StatusColumn
	};

	// Accepts .qc files and folders dropped onto the list.
	class QcList : public QTreeWidget
	{
	public:
		QcList(CompilePage *page, QWidget *parent)
			: QTreeWidget(parent), m_page(page)
		{
			setAcceptDrops(true);
			setDragDropMode(QAbstractItemView::DropOnly);
		}

	protected:
		void dragEnterEvent(QDragEnterEvent *event) override
		{
			if (event->mimeData()->hasUrls())
				event->acceptProposedAction();
		}
		void dragMoveEvent(QDragMoveEvent *event) override
		{
			if (event->mimeData()->hasUrls())
				event->acceptProposedAction();
		}
		void dropEvent(QDropEvent *event) override
		{
			for (const QUrl &url : event->mimeData()->urls())
			{
				if (url.isLocalFile())
					m_page->addPath(url.toLocalFile());
			}
			event->acceptProposedAction();
		}

	private:
		CompilePage *m_page;
	};

} // namespace

CompilePage::CompilePage(GameList *games, TaskManager *tasks, QWidget *parent)
	: Page(tr("Compile"), games, tasks, parent), m_files(new QcList(this, this)), m_outputMode(new QComboBox(this)),
	  m_outputSubfolder(new QLineEdit(this)), m_outputPath(new QLineEdit(this)), m_outputBrowse(new QPushButton(tr("Browse…"), this)),
	  m_noP4(new QCheckBox(tr("No P4 (-nop4)"), this)), m_verbose(new QCheckBox(tr("Verbose (-verbose)"), this)),
	  m_viewAfter(new QCheckBox(tr("Open in HLMV afterwards"), this)), m_extraOptions(new QLineEdit(this)),
	  m_remove(new QPushButton(tr("Remove"), this)), m_view(new QPushButton(tr("Open in HLMV"), this)),
	  m_compileSelected(new QPushButton(tr("Compile Selected"), this)), m_compileAll(new QPushButton(tr("Compile All"), this)),
	  m_cancel(new QPushButton(tr("Cancel"), this))
{
	setUpPathField(m_outputPath, "compile-output", PathKind::Folder);

	const QSettings settings;
	m_files->setHeaderLabels({tr("QC file"), tr("Status")});
	m_files->setRootIsDecorated(false);
	m_files->setSelectionMode(QAbstractItemView::ExtendedSelection);
	m_files->setUniformRowHeights(true);
	m_files->header()->setStretchLastSection(false);
	m_files->header()->setSectionResizeMode(FileColumn, QHeaderView::Stretch);
	m_files->header()->setSectionResizeMode(StatusColumn, QHeaderView::ResizeToContents);
	m_files->setToolTip(tr("Drop .qc files or folders here."));

	m_outputMode->addItem(tr("Game's \"models\" folder"), GameModels);
	m_outputMode->addItem(tr("Subfolder (of QC input)"), QcSubfolder);
	m_outputMode->addItem(tr("Work folder"), WorkFolder);
	m_outputMode->setCurrentIndex(qBound(0, settings.value("compile/outputMode", 0).toInt(), 2));
	m_outputSubfolder->setText(settings.value("compile/outputSubfolder", "compiled").toString());
	m_outputPath->setText(settings.value("compile/outputPath").toString());
	m_outputPath->setPlaceholderText(tr("Folder for compiled files"));
	m_noP4->setChecked(settings.value("compile/nop4", true).toBool());
	m_verbose->setChecked(settings.value("compile/verbose", false).toBool());
	m_viewAfter->setChecked(settings.value("compile/viewAfter", false).toBool());
	m_viewAfter->setToolTip(tr("When a single model was compiled."));
	m_extraOptions->setText(settings.value("compile/options").toString());
	m_extraOptions->setPlaceholderText(tr("Additional studiomdl options"));

	auto *addFile = new QPushButton(QIcon::fromTheme("list-add"), tr("Add QC…"), this);
	auto *addFolder = new QPushButton(QIcon::fromTheme("folder-new"), tr("Add Folder…"), this);
	auto *clear = new QPushButton(tr("Clear"), this);
	auto *listButtons = new QHBoxLayout;
	listButtons->addWidget(addFile);
	listButtons->addWidget(addFolder);
	listButtons->addStretch(1);
	listButtons->addWidget(m_remove);
	listButtons->addWidget(clear);

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
	form->addRow(tr("Output to:"), outputRow);
	form->addRow(tr("Options:"), optionsRow);
	form->addRow(QString(), m_viewAfter);

	auto *buttons = new QHBoxLayout;
	buttons->addStretch(1);
	buttons->addWidget(m_view);
	buttons->addWidget(m_cancel);
	buttons->addWidget(m_compileSelected);
	buttons->addWidget(m_compileAll);
	m_compileAll->setDefault(true);

	content()->addLayout(listButtons);
	content()->addWidget(m_files, 1);
	content()->addLayout(form);
	content()->addLayout(buttons);

	connect(addFile, &QPushButton::clicked, this, &CompilePage::browseFiles);
	connect(addFolder, &QPushButton::clicked, this, &CompilePage::browseFolder);
	connect(m_remove, &QPushButton::clicked, this, [this]
			{
		for (QTreeWidgetItem *item : m_files->selectedItems())
		{
			if (!m_queue.contains(item))
				delete item;
		}
		updateButtons(); });
	connect(clear, &QPushButton::clicked, this, [this]
			{
		if (m_queue.isEmpty() && !m_current)
			m_files->clear();
		updateButtons(); });
	connect(m_outputBrowse, &QPushButton::clicked, this, [this]
			{
		const QString dir = QFileDialog::getExistingDirectory(this, tr("Select output folder"), m_outputPath->text());
		if (!dir.isEmpty())
			m_outputPath->setText(dir); });
	connect(m_outputMode, &QComboBox::currentIndexChanged, this, [this]
			{
		const int mode = m_outputMode->currentData().toInt();
		m_outputSubfolder->setVisible(mode == QcSubfolder);
		m_outputPath->setVisible(mode == WorkFolder);
		m_outputBrowse->setVisible(mode == WorkFolder); });
	emit m_outputMode->currentIndexChanged(m_outputMode->currentIndex());
	connect(m_files, &QTreeWidget::itemSelectionChanged, this, &CompilePage::updateButtons);
	connect(m_files, &QTreeWidget::itemActivated, this, [this](QTreeWidgetItem *item)
			{ openInViewer(compiledModelFor(item->text(FileColumn))); });
	connect(m_compileAll, &QPushButton::clicked, this, [this]
			{ compile(false); });
	connect(m_compileSelected, &QPushButton::clicked, this, [this]
			{ compile(true); });
	connect(m_view, &QPushButton::clicked, this, &CompilePage::viewSelected);
	connect(m_cancel, &QPushButton::clicked, this, [this]
			{
		m_queue.clear();
		if (m_current)
			m_current->cancel();
		updateButtons(); });
	connect(m_games, &GameList::currentChanged, this, &CompilePage::updateButtons);

	addFiles(settings.value("compile/files").toStringList());
	updateButtons();
}

CompilePage::~CompilePage()
{
	saveSettings();
}

void CompilePage::addPath(const QString &path)
{
	const QFileInfo info(path);
	if (info.isDir())
	{
		QStringList found;
		QDirIterator it(info.absoluteFilePath(), {"*.qc"}, QDir::Files, QDirIterator::Subdirectories);
		while (it.hasNext())
			found << it.next();
		found.sort();
		addFiles(found);
	}
	else if (info.isFile())
	{
		addFiles({info.absoluteFilePath()});
	}
}

void CompilePage::addFiles(const QStringList &files)
{
	for (const QString &file : files)
	{
		bool present = false;
		for (int i = 0; i < m_files->topLevelItemCount() && !present; ++i)
			present = m_files->topLevelItem(i)->text(FileColumn) == file;
		if (present || file.isEmpty())
			continue;
		auto *item = new QTreeWidgetItem(m_files, {file, QString()});
		item->setToolTip(FileColumn, file);
		if (!QFileInfo::exists(file))
			item->setText(StatusColumn, tr("missing"));
	}
	updateButtons();
}

void CompilePage::browseFiles()
{
	const QStringList files = QFileDialog::getOpenFileNames(this, tr("Select QC files"), QSettings().value("compile/lastDir").toString(),
															tr("QC files (*.qc);;All files (*)"));
	if (files.isEmpty())
		return;
	QSettings().setValue("compile/lastDir", QFileInfo(files.first()).absolutePath());
	addFiles(files);
}

void CompilePage::browseFolder()
{
	const QString dir = QFileDialog::getExistingDirectory(this, tr("Select a folder with QC files"), QSettings().value("compile/lastDir").toString());
	if (dir.isEmpty())
		return;
	QSettings().setValue("compile/lastDir", dir);
	addPath(dir);
}

QString CompilePage::outputDirFor(const QString &qcFile) const
{
	switch (m_outputMode->currentData().toInt())
	{
	case QcSubfolder:
		return QFileInfo(qcFile).absolutePath() + '/' + m_outputSubfolder->text().trimmed();
	case WorkFolder:
		return m_outputPath->text().trimmed();
	default:
		return {};
	}
}

QString CompilePage::compiledModelFor(const QString &qcFile) const
{
	const auto target = m_games->current();
	const QString modelName = studiomdl::readModelName(qcFile);
	if (!target || modelName.isEmpty())
		return {};
	const QString outputDir = outputDirFor(qcFile);
	const QString base = outputDir.isEmpty() ? target->gameDir : outputDir;
	return launcher::findPathIgnoringCase(base, "models/" + modelName);
}

void CompilePage::compile(bool selectedOnly)
{
	if (m_current || !m_queue.isEmpty())
		return;
	saveSettings();
	m_queue.clear();
	for (int i = 0; i < m_files->topLevelItemCount(); ++i)
	{
		QTreeWidgetItem *item = m_files->topLevelItem(i);
		if (!selectedOnly || item->isSelected())
			m_queue << item;
	}
	m_succeededInRun = 0;
	m_lastCompiled.clear();
	for (QTreeWidgetItem *item : m_queue)
	{
		item->setText(StatusColumn, tr("queued"));
		item->setIcon(StatusColumn, {});
	}
	startNext();
}

void CompilePage::startNext()
{
	m_current = nullptr;
	if (m_queue.isEmpty())
	{
		// Several viewer windows at once would only get in the way.
		if (m_viewAfter->isChecked() && m_succeededInRun == 1)
			openInViewer(compiledModelFor(m_lastCompiled));
		updateButtons();
		return;
	}

	QTreeWidgetItem *item = m_queue.takeFirst();
	const QString qcFile = item->text(FileColumn);
	Task *task = m_tasks->start(tr("Compile %1").arg(QFileInfo(qcFile).fileName()));
	m_current = task;
	setStatus(item, task);
	connect(task, &Task::stateChanged, this, [this, item, task]
			{
		setStatus(item, task);
		if (task->state() == Task::State::Succeeded)
		{
			++m_succeededInRun;
			m_lastCompiled = item->text(FileColumn);
		}
		if (task->state() == Task::State::Cancelled)
		{
			for (QTreeWidgetItem *rest : std::as_const(m_queue))
				rest->setText(StatusColumn, {});
			m_queue.clear();
		}
		startNext(); });
	updateButtons();

	const auto target = m_games->current();
	if (!target)
	{
		task->fail(tr("No game is selected."));
		return;
	}

	studiomdl::Options options;
	options.noP4 = m_noP4->isChecked();
	options.verbose = m_verbose->isChecked();
	options.extra = QProcess::splitCommand(m_extraOptions->text());

	const QString gameDir = target->gameDir;
	const QString outputDir = outputDirFor(qcFile);
	if (m_outputMode->currentData().toInt() == WorkFolder && outputDir.isEmpty())
	{
		task->fail(tr("Select an output folder."));
		return;
	}
	const QString modelName = studiomdl::readModelName(qcFile);
	task->run(studiomdl::compileCommand(target->app, gameDir, qcFile, options), [this, task, gameDir, outputDir, modelName](int, bool success)
			  {
		if (!success || outputDir.isEmpty())
			return success;
		task->appendLine({});
		if (modelName.isEmpty())
		{
			task->appendLine(tr("WARNING: No $modelname found in the QC file; compiled files were left in the game's \"models\" folder."));
			return true;
		}
		QStringList log;
		studiomdl::moveCompiledModel(gameDir, modelName, outputDir, log);
		for (const QString &line : log)
			task->appendLine(line);
		return true; });
}

void CompilePage::viewSelected()
{
	const QList<QTreeWidgetItem *> selected = m_files->selectedItems();
	if (!selected.isEmpty())
		openInViewer(compiledModelFor(selected.first()->text(FileColumn)));
}

void CompilePage::openInViewer(const QString &mdlFile)
{
	const auto target = m_games->current();
	if (!target)
		return;
	Task *task = m_tasks->start(tr("HLMV %1").arg(QFileInfo(mdlFile).fileName()));
	if (mdlFile.isEmpty())
	{
		task->fail(tr("The compiled model was not found. Compile it first."));
		return;
	}
	task->run(hlmv::viewCommand(target->app, target->gameDir, mdlFile), [](int, bool)
			  { return true; });
}

void CompilePage::setStatus(QTreeWidgetItem *item, Task *task)
{
	switch (task->state())
	{
	case Task::State::Running:
		item->setText(StatusColumn, tr("compiling…"));
		item->setIcon(StatusColumn, QIcon::fromTheme("view-refresh"));
		break;
	case Task::State::Succeeded:
		item->setText(StatusColumn, tr("done"));
		item->setIcon(StatusColumn, QIcon::fromTheme("dialog-ok-apply", style()->standardIcon(QStyle::SP_DialogApplyButton)));
		break;
	case Task::State::Failed:
		item->setText(StatusColumn, tr("failed"));
		item->setIcon(StatusColumn, QIcon::fromTheme("dialog-error", style()->standardIcon(QStyle::SP_MessageBoxCritical)));
		break;
	case Task::State::Cancelled:
		item->setText(StatusColumn, tr("cancelled"));
		item->setIcon(StatusColumn, QIcon::fromTheme("process-stop"));
		break;
	}
}

void CompilePage::updateButtons()
{
	const auto target = m_games->current();
	const bool hasTool = target && !target->tool("studiomdl").isEmpty();
	setNotice(!target	 ? tr("No Source game found. Check the Games page.")
			  : !hasTool ? tr("%1 has no studiomdl. Set a tools folder on the Games page.").arg(target->app.name)
						 : QString());

	const bool busy = m_current || !m_queue.isEmpty();
	const bool any = m_files->topLevelItemCount() > 0;
	const bool selected = !m_files->selectedItems().isEmpty();
	m_compileAll->setEnabled(!busy && any && hasTool);
	m_compileSelected->setEnabled(!busy && selected && hasTool);
	m_cancel->setEnabled(busy);
	m_remove->setEnabled(selected);
	m_view->setEnabled(selected && target && !target->tool("hlmv").isEmpty());
}

void CompilePage::saveSettings() const
{
	QStringList files;
	for (int i = 0; i < m_files->topLevelItemCount(); ++i)
		files << m_files->topLevelItem(i)->text(FileColumn);
	rememberPath("compile-output", m_outputPath->text());

	QSettings settings;
	settings.setValue("compile/files", files);
	settings.setValue("compile/outputMode", m_outputMode->currentIndex());
	settings.setValue("compile/outputSubfolder", m_outputSubfolder->text());
	settings.setValue("compile/outputPath", m_outputPath->text());
	settings.setValue("compile/nop4", m_noP4->isChecked());
	settings.setValue("compile/verbose", m_verbose->isChecked());
	settings.setValue("compile/viewAfter", m_viewAfter->isChecked());
	settings.setValue("compile/options", m_extraOptions->text());
}
