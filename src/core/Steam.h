#pragma once

#include <QList>
#include <QString>
#include <QStringList>

namespace steam {

enum class Platform { Linux, Windows };

struct CompatTool {
    QString name;
    QString displayName;
    QString path;
};

struct App {
    quint32 id = 0;
    QString name;
    QString installPath;
    QString steamRoot;
    Platform platform = Platform::Linux;
    QStringList gameDirs;
    QString toolsPath;
    QString compatDataPath;
    QString compatToolName;
    QString protonPath;

    bool isSourceGame() const { return !gameDirs.isEmpty(); }
};

struct Installation {
    QString root;
    QStringList libraries;
    QList<CompatTool> compatTools;
    QList<App> apps;
};

QStringList findRoots();
Installation scan(const QString &root);
QList<Installation> scanAll();

// Internal name Steam uses in CompatToolMapping for a Valve-shipped Proton
// directory, e.g. "Proton 9.0 (Beta)" -> "proton_9". Empty if not recognised.
QString officialToolName(const QString &directoryName);

} // namespace steam
