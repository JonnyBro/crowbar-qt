#pragma once

#include "core/Steam.h"

#include <QObject>

class GameList : public QObject {
    Q_OBJECT

public:
    using QObject::QObject;

    void refresh();

    // Empty toolsPath goes back to the tools found in the game folder.
    static void setToolsOverride(quint32 appId, const QString &toolsPath);
    static QString toolsOverride(quint32 appId);

    bool steamFound() const { return m_steamFound; }
    const QList<steam::App> &apps() const { return m_apps; }
    const QStringList &libraries() const { return m_libraries; }

signals:
    void changed();

private:
    QList<steam::App> m_apps;
    QStringList m_libraries;
    bool m_steamFound = false;
};
