#pragma once

#include "core/Workshop.h"
#include "ui/Page.h"

#include <QTemporaryDir>

class QCheckBox;
class QLabel;
class QLineEdit;
class QPushButton;
class Task;

class DownloadPage : public Page {
    Q_OBJECT

public:
    DownloadPage(GameList *games, TaskManager *tasks, QWidget *parent = nullptr);
    ~DownloadPage() override;

private:
    void fetch(bool thenDownload);
    void showDetails(const workshop::ItemDetails &details);
    void download();
    void save(Task *task, const QStringList &files);
    void saveSettings() const;

    workshop::Client *m_client;
    workshop::ItemDetails m_details;
    bool m_downloadAfterFetch = false;
    QTemporaryDir m_tempDir;

    QLineEdit *m_item;
    QLabel *m_preview;
    QLabel *m_info;
    QLineEdit *m_outputPath;
    QCheckBox *m_withTitle;
    QCheckBox *m_convert;
    QPushButton *m_download;
};
