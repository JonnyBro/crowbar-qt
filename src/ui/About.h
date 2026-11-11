#pragma once

#include <QString>

class QWidget;

// "1.2.0", or "1.2.0 (g0ee422d)" for a build that is not the release itself.
QString versionString();

void showAboutDialog(QWidget *parent);
