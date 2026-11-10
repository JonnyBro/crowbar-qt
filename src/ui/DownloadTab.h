#pragma once

#include "core/Workshop.h"

#include <QTemporaryDir>
#include <QWidget>

class GameList;
class QCheckBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QProgressBar;
class QPushButton;
class ToolProcess;

class DownloadTab : public QWidget {
    Q_OBJECT

public:
    explicit DownloadTab(GameList *games, QWidget *parent = nullptr);
    ~DownloadTab() override;

private:
    void fetch(bool thenDownload);
    void showDetails(const workshop::ItemDetails &details);
    void download();
    void finish(const QStringList &files);
    void setBusy(bool busy);
    void saveSettings() const;

    GameList *m_games;
    workshop::Client *m_client;
    ToolProcess *m_steamCmd;
    workshop::ItemDetails m_details;
    bool m_downloadAfterFetch = false;
    bool m_busy = false;
    QTemporaryDir m_tempDir;
    QString m_steamCmdFolder;

    QLineEdit *m_item;
    QLabel *m_info;
    QLineEdit *m_outputPath;
    QCheckBox *m_withTitle;
    QCheckBox *m_convert;
    QPushButton *m_download;
    QPushButton *m_cancel;
    QPushButton *m_openPage;
    QProgressBar *m_progress;
    QPlainTextEdit *m_log;
};
