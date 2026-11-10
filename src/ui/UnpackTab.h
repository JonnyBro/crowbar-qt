#pragma once

#include "core/Package.h"

#include <QTemporaryDir>
#include <QWidget>

#include <optional>

class QComboBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QProgressBar;
class QPushButton;
class QTreeWidget;
class QTreeWidgetItem;

class UnpackTab : public QWidget {
    Q_OBJECT

public:
    explicit UnpackTab(QWidget *parent = nullptr);
    ~UnpackTab() override;

private:
    void browse();
    void browseOutput();
    void loadPackage();
    void updateOutputWidgets();
    QString outputDir() const;
    void unpack(bool selectedOnly);
    void openItem(QTreeWidgetItem *item);
    void setRunning(bool running);
    void saveSettings() const;

    std::optional<package::Package> m_package;
    package::ExtractJob *m_job = nullptr;
    QTemporaryDir m_tempDir;

    QLineEdit *m_file;
    QLabel *m_info;
    QTreeWidget *m_tree;
    QComboBox *m_outputMode;
    QLineEdit *m_outputPath;
    QPushButton *m_outputBrowse;
    QPushButton *m_unpackAll;
    QPushButton *m_unpackSelected;
    QPushButton *m_cancel;
    QProgressBar *m_progress;
    QPlainTextEdit *m_log;
};
