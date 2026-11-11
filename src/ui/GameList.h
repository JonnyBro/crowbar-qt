#pragma once

#include "core/Steam.h"

#include <QObject>

#include <optional>

// Installed Source games plus the one the whole window works with.
class GameList : public QObject {
    Q_OBJECT

public:
    struct Target {
        steam::App app;
        QString gameDir;

        // Path of a tool from the game's tools folder, or empty.
        QString tool(const QString &name) const;
    };

    using QObject::QObject;

    void refresh();

    // Every game folder of every game, in display order.
    QList<Target> targets() const;
    std::optional<Target> current() const;
    void setCurrent(const QString &gameDir);

    // Empty toolsPath goes back to the tools found in the game folder.
    static void setToolsOverride(quint32 appId, const QString &toolsPath);
    static QString toolsOverride(quint32 appId);

    // Game folders added by hand (a gameinfo.txt folder outside Steam's lists).
    static QStringList customFolders();
    static void addCustomFolder(const QString &gameDir);
    static void removeCustomFolder(const QString &gameDir);

    bool steamFound() const { return m_steamFound; }
    const QList<steam::App> &apps() const { return m_apps; }
    const QStringList &libraries() const { return m_libraries; }
    // Every Proton build found, for choosing one on the settings page.
    const QList<steam::CompatTool> &compatTools() const { return m_compatTools; }

signals:
    void changed();
    void currentChanged();

private:
    QList<steam::App> m_apps;
    QStringList m_libraries;
    QList<steam::CompatTool> m_compatTools;
    bool m_steamFound = false;
    QString m_currentDir;
};
