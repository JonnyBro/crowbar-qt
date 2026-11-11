#pragma once

#include <QString>

class QLineEdit;

enum class PathKind { File, Folder, Any };

// Turns a line edit into a path field: path completion while typing, a
// drop-down of recently used paths, and dropping a file or folder onto it.
void setUpPathField(QLineEdit *edit, const QString &historyKey, PathKind kind);

// Adds a path to the field's history; call it when the path is actually used.
void rememberPath(const QString &historyKey, const QString &path);
