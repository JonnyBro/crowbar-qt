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
    void setCurrent(quint32 appId, const QString &gameDir);

    // Empty toolsPath goes back to the tools found in the game folder.
    static void setToolsOverride(quint32 appId, const QString &toolsPath);
    static QString toolsOverride(quint32 appId);

    bool steamFound() const { return m_steamFound; }
    const QList<steam::App> &apps() const { return m_apps; }
    const QStringList &libraries() const { return m_libraries; }

signals:
    void changed();
    void currentChanged();

private:
    QList<steam::App> m_apps;
    QStringList m_libraries;
    bool m_steamFound = false;
    quint32 m_currentApp = 0;
    QString m_currentDir;
};
