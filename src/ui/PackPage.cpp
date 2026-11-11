#include "ui/PackPage.h"

#include "core/Packer.h"
#include "ui/GameList.h"
#include "ui/PathField.h"
#include "ui/Tasks.h"

#include <QCheckBox>
#include <QComboBox>
#include <QDateTime>
#include <QDir>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QProcess>
#include <QPushButton>
#include <QSettings>
#include <QVBoxLayout>

namespace
{

	enum OutputMode
	{
		ParentFolder,
		WorkFolder
	};

} // namespace

PackPage::PackPage(GameList *games, TaskManager *tasks, QWidget *parent)
	: Page(tr("Pack"), games, tasks, parent), m_format(new QComboBox(this)), m_form(new QFormLayout), m_folder(new QLineEdit(this)),
	  m_outputMode(new QComboBox(this)), m_outputPath(new QLineEdit(this)), m_outputBrowse(new QPushButton(tr("Browse…"), this)),
	  m_multiChunk(new QCheckBox(tr("Multi-file VPK (-M)"), this)), m_warnInvalid(new QCheckBox(tr("Skip invalid files (-warninvalid)"), this)),
	  m_extraOptions(new QLineEdit(this)), m_gmaRow(new QWidget(this)), m_gmaTitle(new QLineEdit(this)), m_gmaType(new QComboBox(this)),
	  m_gmaTag1(new QComboBox(this)), m_gmaTag2(new QComboBox(this)), m_addonStatus(new QLabel(this)),
	  m_saveAddon(new QPushButton(QIcon::fromTheme("document-save"), tr("Save to addon.json"), this)), m_pack(new QPushButton(tr("Pack"), this))
{
	setUpPathField(m_folder, "pack-folder", PathKind::Folder);
	setUpPathField(m_outputPath, "pack-output", PathKind::Folder);

	const QSettings settings;
	m_format->addItem(tr("GMA (gmad)"), int(packer::Format::Gma));
	m_format->addItem(tr("VPK (vpk)"), int(packer::Format::Vpk));
	m_format->setCurrentIndex(qMax(0, m_format->findData(settings.value("pack/formatId", int(packer::Format::Gma)))));
	m_folder->setText(settings.value("pack/folder").toString());
	m_folder->setPlaceholderText(tr("Folder to pack"));
	m_outputMode->addItem(tr("Parent of input folder"), ParentFolder);
	m_outputMode->addItem(tr("Work folder"), WorkFolder);
	m_outputMode->setCurrentIndex(qBound(0, settings.value("pack/outputMode", 0).toInt(), 1));
	m_outputPath->setText(settings.value("pack/outputPath").toString());
	m_outputPath->setPlaceholderText(tr("Folder for the packed file"));
	m_multiChunk->setChecked(settings.value("pack/multiChunk", false).toBool());
	m_warnInvalid->setChecked(settings.value("pack/warnInvalid", true).toBool());
	m_extraOptions->setText(settings.value("pack/options").toString());
	m_extraOptions->setPlaceholderText(tr("Additional packer options"));

	m_gmaTitle->setText(settings.value("pack/gmaTitle").toString());
	m_gmaTitle->setPlaceholderText(tr("Title (defaults to the folder name)"));
	m_gmaType->addItems(packer::addonTypes());
	m_gmaType->setCurrentText(settings.value("pack/gmaType", "model").toString());
	for (QComboBox *tag : {m_gmaTag1, m_gmaTag2})
	{
		tag->addItem(tr("(no tag)"), QString());
		for (const QString &name : packer::addonTags())
			tag->addItem(name, name);
	}
	m_gmaTag1->setCurrentIndex(qMax(0, m_gmaTag1->findData(settings.value("pack/gmaTag1").toString())));
	m_gmaTag2->setCurrentIndex(qMax(0, m_gmaTag2->findData(settings.value("pack/gmaTag2").toString())));
	m_gmaRow->setToolTip(tr("Used to create addon.json when the folder does not have one yet."));

	auto *browse = new QPushButton(tr("Browse…"), this);
	auto *folderRow = new QHBoxLayout;
	folderRow->addWidget(m_folder, 1);
	folderRow->addWidget(browse);

	auto *outputRow = new QHBoxLayout;
	outputRow->addWidget(m_outputMode);
	outputRow->addWidget(m_outputPath, 1);
	outputRow->addWidget(m_outputBrowse);

	auto *optionsRow = new QHBoxLayout;
	optionsRow->addWidget(m_multiChunk);
	optionsRow->addWidget(m_warnInvalid);
	optionsRow->addWidget(m_extraOptions, 1);

	auto *gmaLayout = new QHBoxLayout(m_gmaRow);
	gmaLayout->setContentsMargins(0, 0, 0, 0);
	gmaLayout->addWidget(m_gmaTitle, 1);
	gmaLayout->addWidget(m_gmaType);
	gmaLayout->addWidget(m_gmaTag1);
	gmaLayout->addWidget(m_gmaTag2);
	gmaLayout->addWidget(m_saveAddon);

	m_form->addRow(tr("Format:"), m_format);
	m_form->addRow(tr("Folder:"), folderRow);
	m_form->addRow(tr("Output to:"), outputRow);
	m_form->addRow(tr("Options:"), optionsRow);
	m_form->addRow(tr("addon.json:"), m_gmaRow);
	m_form->addRow(QString(), m_addonStatus);
	m_addonStatus->setForegroundRole(QPalette::PlaceholderText);
	m_addonStatus->setWordWrap(true);
	m_saveAddon->setToolTip(tr("Writes the title, type and tags into the folder's addon.json, keeping its other settings."));

	auto *buttons = new QHBoxLayout;
	buttons->addStretch(1);
	buttons->addWidget(m_pack);
	m_pack->setDefault(true);

	content()->addLayout(m_form);
	content()->addLayout(buttons);
	content()->addStretch(1);

	connect(browse, &QPushButton::clicked, this, [this]
			{
		const QString dir = QFileDialog::getExistingDirectory(this, tr("Select folder to pack"), m_folder->text());
		if (!dir.isEmpty())
			setFolder(dir); });
	connect(m_outputBrowse, &QPushButton::clicked, this, [this]
			{
		const QString dir = QFileDialog::getExistingDirectory(this, tr("Select output folder"), m_outputPath->text());
		if (!dir.isEmpty())
			m_outputPath->setText(dir); });
	connect(m_outputMode, &QComboBox::currentIndexChanged, this, &PackPage::updateWidgets);
	connect(m_format, &QComboBox::currentIndexChanged, this, &PackPage::updateWidgets);
	connect(m_games, &GameList::currentChanged, this, &PackPage::updateWidgets);
	connect(m_pack, &QPushButton::clicked, this, &PackPage::pack);
	connect(m_folder, &QLineEdit::editingFinished, this, &PackPage::loadAddonJson);
	connect(m_saveAddon, &QPushButton::clicked, this, &PackPage::saveAddonJson);
	updateWidgets();
	loadAddonJson();
}

PackPage::~PackPage()
{
	saveSettings();
}

void PackPage::setFolder(const QString &folder)
{
	m_folder->setText(folder);
	loadAddonJson();
}

void PackPage::loadAddonJson()
{
	const QString folder = QDir::cleanPath(m_folder->text().trimmed());
	if (folder == m_loadedFolder && !folder.isEmpty())
		return;
	m_loadedFolder = folder;
	m_loadedInfo.reset();
	if (folder.isEmpty() || !QFileInfo(folder).isDir())
	{
		m_addonStatus->clear();
		return;
	}
	if (!packer::hasAddonJson(folder))
	{
		m_addonStatus->setText(tr("The folder has no addon.json; it will be created from these fields."));
		return;
	}

	QString error;
	const auto info = packer::readAddonJson(folder, &error);
	if (!info)
	{
		m_addonStatus->setText(tr("addon.json could not be read, so it is used as is: %1").arg(error));
		return;
	}
	m_loadedInfo = info;
	m_gmaTitle->setText(info->title);
	// Keep values gmad does not know visible, so they are not silently replaced.
	if (!info->type.isEmpty() && m_gmaType->findText(info->type) < 0)
		m_gmaType->addItem(info->type);
	if (!info->type.isEmpty())
		m_gmaType->setCurrentText(info->type);
	const QList<QComboBox *> tags{m_gmaTag1, m_gmaTag2};
	for (int i = 0; i < tags.size(); ++i)
	{
		const QString tag = i < info->tags.size() ? info->tags[i] : QString();
		if (!tag.isEmpty() && tags[i]->findData(tag) < 0)
			tags[i]->addItem(tag, tag);
		tags[i]->setCurrentIndex(qMax(0, tags[i]->findData(tag)));
	}
	m_addonStatus->setText(info->tags.size() > 2 ? tr("Loaded from addon.json. It has more than two tags; gmad accepts only two.")
												 : tr("Loaded from addon.json."));
}

packer::AddonInfo PackPage::addonInfo(const QString &folder) const
{
	packer::AddonInfo info;
	info.title = m_gmaTitle->text().trimmed();
	if (info.title.isEmpty())
		info.title = QFileInfo(folder).fileName();
	info.type = m_gmaType->currentText();
	for (const QComboBox *tag : {m_gmaTag1, m_gmaTag2})
	{
		const QString name = tag->currentData().toString();
		if (!name.isEmpty() && !info.tags.contains(name))
			info.tags << name;
	}
	return info;
}

bool PackPage::saveAddonJson()
{
	const QString folder = QDir::cleanPath(m_folder->text().trimmed());
	if (folder.isEmpty() || !QFileInfo(folder).isDir())
		return false;
	QString error;
	const packer::AddonInfo info = addonInfo(folder);
	if (!packer::saveAddonJson(folder, info, &error))
	{
		m_addonStatus->setText(tr("Could not save addon.json: %1").arg(error));
		return false;
	}
	m_loadedInfo = info;
	m_addonStatus->setText(tr("Saved to addon.json."));
	return true;
}

void PackPage::updateWidgets()
{
	const auto format = packer::Format(m_format->currentData().toInt());
	const bool gma = format == packer::Format::Gma;
	const bool workFolder = m_outputMode->currentData().toInt() == WorkFolder;
	m_outputPath->setVisible(workFolder);
	m_outputBrowse->setVisible(workFolder);
	m_multiChunk->setVisible(!gma);
	m_warnInvalid->setVisible(gma);
	m_form->setRowVisible(m_gmaRow, gma);
	m_form->setRowVisible(m_addonStatus, gma);

	const auto target = m_games->current();
	const QString tool = packer::toolName(format);
	const bool hasTool = target && !target->tool(tool).isEmpty();
	setNotice(!target		 ? tr("No Source game found. Check the Games page.")
			  : !hasTool ? tr("%1 has no %2. Set a tools folder on the Games page.").arg(target->app.name, tool)
						 : QString());
	m_pack->setEnabled(hasTool && !m_current);
}

bool PackPage::prepareAddonJson(const QString &folder, Task *task)
{
	loadAddonJson();
	const packer::AddonInfo info = addonInfo(folder);
	if (packer::hasAddonJson(folder))
	{
		// Unreadable files are the user's to fix; gmad reports what is wrong with them.
		if (!m_loadedInfo || *m_loadedInfo == info)
		{
			task->appendLine(tr("Using the existing addon.json."));
			return true;
		}
		const auto answer = QMessageBox::question(this, tr("Pack"), tr("The addon.json fields differ from the file. Save them to addon.json before packing?"),
												  QMessageBox::Save | QMessageBox::Discard | QMessageBox::Cancel, QMessageBox::Save);
		if (answer == QMessageBox::Cancel)
		{
			task->appendLine(tr("Cancelled."));
			task->finish(Task::State::Cancelled);
			return false;
		}
		if (answer == QMessageBox::Discard)
		{
			task->appendLine(tr("Using the existing addon.json."));
			return true;
		}
	}
	QString error;
	const bool existed = packer::hasAddonJson(folder);
	if (!packer::saveAddonJson(folder, info, &error))
	{
		task->fail(tr("Could not save addon.json: %1").arg(error));
		return false;
	}
	m_loadedInfo = info;
	task->appendLine((existed ? tr("Updated addon.json (\"%1\", %2).") : tr("Created addon.json (\"%1\", %2).")).arg(info.title, info.type));
	return true;
}

void PackPage::pack()
{
	const auto target = m_games->current();
	if (!target || m_current)
		return;
	saveSettings();

	const QString folder = QDir::cleanPath(m_folder->text().trimmed());
	const QFileInfo folderInfo(folder);
	const auto format = packer::Format(m_format->currentData().toInt());
	Task *task = m_tasks->start(tr("Pack %1").arg(folderInfo.fileName() + u'.' + packer::extension(format)));
	m_current = task;
	connect(task, &Task::stateChanged, this, [this]
			{
		m_current = nullptr;
		updateWidgets(); });
	updateWidgets();

	const bool workFolder = m_outputMode->currentData().toInt() == WorkFolder;
	const QString outputDir = workFolder ? m_outputPath->text().trimmed() : folderInfo.absolutePath();
	if (m_folder->text().trimmed().isEmpty() || outputDir.isEmpty())
	{
		task->fail(tr("Select a folder to pack and an output folder."));
		return;
	}
	if (format == packer::Format::Gma && folderInfo.isDir() && !prepareAddonJson(folder, task))
		return;

	packer::Options options;
	options.multiChunk = format == packer::Format::Vpk && m_multiChunk->isChecked();
	options.warnInvalid = m_warnInvalid->isChecked();
	options.extra = QProcess::splitCommand(m_extraOptions->text());
	const QString outputFile = outputDir + u'/' + folderInfo.fileName() + u'.' + packer::extension(format);
	const QDateTime started = QDateTime::currentDateTime();
	task->run(packer::packCommand(target->app, format, folder, outputFile, options),
			  [this, task, format, folder, outputFile, options, started](int, bool success)
			  {
		if (!success)
			return false;
		QStringList log;
		const QStringList files = packer::finishPack(format, folder, outputFile, options, started, log);
		for (const QString &line : log)
			task->appendLine(line);
		if (!files.isEmpty())
			task->appendLine(tr("\nPacked:\n%1").arg(files.join(u'\n')));
		return !files.isEmpty(); });
}

void PackPage::saveSettings() const
{
	rememberPath("pack-folder", m_folder->text());
	rememberPath("pack-output", m_outputPath->text());
	QSettings settings;
	settings.setValue("pack/formatId", m_format->currentData());
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
