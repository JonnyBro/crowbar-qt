#pragma once

#include <QWidget>

class GameCombo;
class GameList;
class QCheckBox;
class QComboBox;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class ToolProcess;

class CompileTab : public QWidget {
    Q_OBJECT

public:
    explicit CompileTab(GameList *games, QWidget *parent = nullptr);
    ~CompileTab() override;

private:
    void browse();
    void browseOutput();
    void updateOutputWidgets();
    QString outputDir() const;
    void moveOutput();
    void compile();
    void setRunning(bool running);
    void saveSettings() const;

    ToolProcess *m_process;
    QString m_pendingGameDir;
    QString m_pendingModelName;
    QString m_pendingOutputDir;

    GameCombo *m_game;
    QLineEdit *m_qcFile;
    QComboBox *m_outputMode;
    QLineEdit *m_outputSubfolder;
    QLineEdit *m_outputPath;
    QPushButton *m_outputBrowse;
    QCheckBox *m_noP4;
    QCheckBox *m_verbose;
    QLineEdit *m_extraOptions;
    QPushButton *m_compile;
    QPushButton *m_cancel;
    QPlainTextEdit *m_log;
};
