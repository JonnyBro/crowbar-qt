#include "ui/GameList.h"

void GameList::refresh()
{
    const QList<steam::Installation> installations = steam::scanAll();
    m_steamFound = !installations.isEmpty();
    m_apps.clear();
    for (const steam::Installation &installation : installations) {
        for (const steam::App &app : installation.apps) {
            if (app.isSourceGame())
                m_apps << app;
        }
    }
    emit changed();
}
