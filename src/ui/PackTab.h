#pragma once

#include <QDateTime>
#include <QWidget>

class GameCombo;
class GameList;
class QCheckBox;
class QFormLayout;
class QComboBox;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class ToolProcess;

class PackTab : public QWidget {
    Q_OBJECT

public:
    explicit PackTab(GameList *games, QWidget *parent = nullptr);
    ~PackTab() override;

private:
    void browse();
    void browseOutput();
    void updateOutputWidgets();
    void updateFormatWidgets();
    bool prepareAddonJson(const QString &folder);
    void pack();
    void setRunning(bool running);
    void saveSettings() const;

    ToolProcess *m_process;
    QString m_pendingFolder;
    QDateTime m_started;
    bool m_pendingMultiChunk = false;
    QString m_pendingOutputFile;

    GameCombo *m_game;
    QComboBox *m_format;
    QFormLayout *m_form;
    QCheckBox *m_multiChunk;
    QCheckBox *m_warnInvalid;
    QLineEdit *m_extraOptions;
    QWidget *m_gmaRow;
    QLineEdit *m_gmaTitle;
    QComboBox *m_gmaType;
    QComboBox *m_gmaTag1;
    QComboBox *m_gmaTag2;
    QLineEdit *m_folder;
    QComboBox *m_outputMode;
    QLineEdit *m_outputPath;
    QPushButton *m_outputBrowse;
    QPushButton *m_pack;
    QPushButton *m_cancel;
    QPlainTextEdit *m_log;
};
