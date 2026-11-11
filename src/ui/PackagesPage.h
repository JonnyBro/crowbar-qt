#pragma once

#include "core/Package.h"
#include "ui/Page.h"

#include <optional>

class QComboBox;
class Task;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QScrollArea;
class QStackedWidget;
class QTimer;
class QTreeWidget;
class QTreeWidgetItem;

// Browses a VPK or GMA: search, preview, drag files out, unpack.
class PackagesPage : public Page
{
	Q_OBJECT

public:
	PackagesPage(GameList *games, TaskManager *tasks, QWidget *parent = nullptr);
	~PackagesPage() override;

	void openPackage(const QString &file);
	// Opens the package and selects one of its files.
	void openEntry(const QString &file, const QString &entryPath);

private:
	void loadPackage();
	void applyFilter();
	void showPreview();
	void openItem(QTreeWidgetItem *item);
	QString outputDir() const;
	void unpack(bool selectedOnly);
	QList<int> selectedEntries() const;
	// Extracts entries into the temporary folder; returns their paths there.
	QStringList extractToTemp(const QList<int> &indexes);
	void updateButtons();
	void updateScope();
	void searchAllPackages();
	void addResults(const QString &package, const QList<QPair<QString, quint64>> &matches);
	void saveSettings() const;

	std::optional<package::Package> m_package;
	bool m_unpacking = false;

	QLineEdit *m_file;
	QLabel *m_info;
	QLineEdit *m_filter;
	QTimer *m_filterTimer;
	QComboBox *m_scope;
	QPushButton *m_search;
	QStackedWidget *m_left;
	QTreeWidget *m_results;
	Task *m_searchTask = nullptr;
	int m_resultCount = 0;
	QTreeWidget *m_tree;
	QStackedWidget *m_preview;
	QLabel *m_previewMessage;
	QPlainTextEdit *m_previewText;
	QScrollArea *m_previewImageArea;
	QLabel *m_previewImage;
	QComboBox *m_outputMode;
	QLineEdit *m_outputPath;
	QPushButton *m_outputBrowse;
	QPushButton *m_unpackAll;
	QPushButton *m_unpackSelected;
};
