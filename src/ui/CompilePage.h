#pragma once

#include "ui/Page.h"

class QCheckBox;
class QComboBox;
class QLineEdit;
class QPushButton;
class QTreeWidget;
class QTreeWidgetItem;
class Task;

class CompilePage : public Page
{
	Q_OBJECT

public:
	CompilePage(GameList *games, TaskManager *tasks, QWidget *parent = nullptr);
	~CompilePage() override;

	// Adds a .qc file, or every .qc under a folder.
	void addPath(const QString &path);

private:
	void addFiles(const QStringList &files);
	void browseFiles();
	void browseFolder();
	void compile(bool selectedOnly);
	void startNext();
	void viewSelected();
	QString outputDirFor(const QString &qcFile) const;
	QString compiledModelFor(const QString &qcFile) const;
	void openInViewer(const QString &mdlFile);
	void setStatus(QTreeWidgetItem *item, Task *task);
	void updateButtons();
	void saveSettings() const;

	QList<QTreeWidgetItem *> m_queue;
	Task *m_current = nullptr;
	int m_succeededInRun = 0;
	QString m_lastCompiled;

	QTreeWidget *m_files;
	QComboBox *m_outputMode;
	QLineEdit *m_outputSubfolder;
	QLineEdit *m_outputPath;
	QPushButton *m_outputBrowse;
	QCheckBox *m_noP4;
	QCheckBox *m_verbose;
	QCheckBox *m_viewAfter;
	QLineEdit *m_extraOptions;
	QPushButton *m_remove;
	QPushButton *m_view;
	QPushButton *m_compileSelected;
	QPushButton *m_compileAll;
	QPushButton *m_cancel;
};
