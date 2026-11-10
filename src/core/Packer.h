#pragma once

#include "core/Launcher.h"

#include <QDateTime>

namespace packer {

enum class Format { Vpk, Gma };

struct Options {
    // VPK only: split into <name>_dir.vpk plus numbered chunks (vpk -M).
    bool multiChunk = false;
    // GMA only: skip files gmad does not allow instead of failing (-warninvalid).
    bool warnInvalid = false;
    // Passed to the packer as is.
    QStringList extra;
};

QString toolName(Format format);
QString extension(Format format);

// GMA: gmad writes straight to outputFile.
// VPK: vpk has no output argument and always writes next to the input folder;
// finishPack() moves the result to where outputFile says.
launcher::Command packCommand(const steam::App &app, Format format, const QString &folder,
                              const QString &outputFile, const Options &options = {});

// Call after the packer exits successfully. Returns the final package files,
// or an empty list if the packer produced nothing. `started` keeps chunks left
// over from an earlier, larger multi-chunk build from being picked up.
QStringList finishPack(Format format, const QString &folder, const QString &outputFile,
                       const Options &options, const QDateTime &started, QStringList &log);

// Values gmad accepts in addon.json.
QStringList addonTypes();
QStringList addonTags();

bool hasAddonJson(const QString &folder);
bool writeAddonJson(const QString &folder, const QString &title, const QString &type,
                    const QStringList &tags, QString *error = nullptr);

} // namespace packer
