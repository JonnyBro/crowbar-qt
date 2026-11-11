#pragma once

#include "core/Workshop.h"
#include "ui/Page.h"


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
    void showInfo();
    void showDetails(const workshop::ItemDetails &details, int collectionSize);
    // Expands collections, then downloads every item one after another.
    void startDownloads();
    void startNext();
    void download(const workshop::ItemDetails &details);
    void save(Task *task, const workshop::ItemDetails &details, const QStringList &files);
    void saveSettings() const;

    workshop::Client *m_client;
    quint64 m_shownId = 0;
    QList<workshop::ItemDetails> m_queue;
    Task *m_current = nullptr;
    bool m_resolving = false;

    QLineEdit *m_item;
    QLabel *m_preview;
    QLabel *m_info;
    QLineEdit *m_outputPath;
    QCheckBox *m_withTitle;
    QCheckBox *m_convert;
    QPushButton *m_download;
};
