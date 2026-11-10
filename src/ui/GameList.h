#pragma once

#include "core/Steam.h"

#include <QObject>

class GameList : public QObject {
    Q_OBJECT

public:
    using QObject::QObject;

    void refresh();

    bool steamFound() const { return m_steamFound; }
    const QList<steam::App> &apps() const { return m_apps; }

signals:
    void changed();

private:
    QList<steam::App> m_apps;
    bool m_steamFound = false;
};
