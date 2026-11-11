#pragma once

#include "core/Packer.h"
#include "ui/Page.h"

#include <optional>

class QCheckBox;
class QComboBox;
class QFormLayout;
class QLabel;
class QLineEdit;
class QPushButton;
class Task;

class PackPage : public Page
{
	Q_OBJECT

public:
	PackPage(GameList *games, TaskManager *tasks, QWidget *parent = nullptr);
	~PackPage() override;

	void setFolder(const QString &folder);

private:
	void updateWidgets();
	bool prepareAddonJson(const QString &folder, Task *task);
	// Fills the addon.json fields from the folder's file, if it has one.
	void loadAddonJson();
	packer::AddonInfo addonInfo(const QString &folder) const;
	bool saveAddonJson();
	void pack();
	void saveSettings() const;

	Task *m_current = nullptr;

	QComboBox *m_format;
	QFormLayout *m_form;
	QLineEdit *m_folder;
	QComboBox *m_outputMode;
	QLineEdit *m_outputPath;
	QPushButton *m_outputBrowse;
	QCheckBox *m_multiChunk;
	QCheckBox *m_warnInvalid;
	QLineEdit *m_extraOptions;
	QWidget *m_gmaRow;
	QLineEdit *m_gmaTitle;
	QComboBox *m_gmaType;
	QComboBox *m_gmaTag1;
	QComboBox *m_gmaTag2;
	QLabel *m_addonStatus;
	QPushButton *m_saveAddon;
	QString m_loadedFolder;
	std::optional<packer::AddonInfo> m_loadedInfo;
	QPushButton *m_pack;
};
