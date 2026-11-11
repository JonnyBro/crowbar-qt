#pragma once

#include <QPlainTextEdit>

// Read-only tool log. Errors and warnings are coloured, and paths that exist
// on disk become links: text files open in the editor (at the line, if the
// message has one), other files are shown in their folder.
class LogView : public QPlainTextEdit {
    Q_OBJECT

public:
    explicit LogView(QWidget *parent = nullptr);

    void appendLine(const QString &line);

protected:
    void mouseMoveEvent(QMouseEvent *event) override;
    void mouseReleaseEvent(QMouseEvent *event) override;
};

// Opens a file in the configured editor, at `line` when the editor supports it.
void openInEditor(const QString &file, int line = 0);
