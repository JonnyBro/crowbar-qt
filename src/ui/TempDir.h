#pragma once

#include <QString>

// The app's scratch folder, shared by all pages: created on first use, removed
// when the app exits. Returns a path under it, creating `subfolder` if given.
// Empty if the folder could not be created.
QString tempPath(const QString &subfolder = {});

// Removes scratch folders left by earlier runs that were killed or crashed.
void removeStaleTempDirs();
