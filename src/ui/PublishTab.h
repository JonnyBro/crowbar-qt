#pragma once

#include "core/Publisher.h"

#include <QTemporaryDir>
#include <QWidget>

class GameCombo;
class GameList;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QTreeWidget;
class ToolProcess;

class PublishTab : public QWidget {
    Q_OBJECT

public:
    explicit PublishTab(GameList *games, QWidget *parent = nullptr);
    ~PublishTab() override;

private:
    enum class Stage { Idle, Listing, Packing, Publishing };

    void refreshList();
    void browseContent();
    void browseIcon();
    void publish();
    void startUpload(const QString &gmaFile);
    void onLine(const QString &line);
    void onFinished(int exitCode, bool success);
    void setStage(Stage stage);
    void saveSettings() const;

    ToolProcess *m_process;
    Stage m_stage = Stage::Idle;
    publisher::ListParser m_parser;
    publisher::Request m_request;
    QTemporaryDir m_tempDir;
    QString m_packedFile;
    quint64 m_createdId = 0;

    GameCombo *m_game;
    QTreeWidget *m_items;
    QPushButton *m_refresh;
    QLineEdit *m_itemId;
    QLabel *m_itemHint;
    QLineEdit *m_content;
    QLineEdit *m_icon;
    QLineEdit *m_changes;
    QPushButton *m_publish;
    QPushButton *m_cancel;
    QPlainTextEdit *m_log;
};
