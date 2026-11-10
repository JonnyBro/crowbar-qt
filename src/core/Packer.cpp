#include "core/Packer.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QRegularExpression>

namespace packer {

namespace {

bool moveFile(const QString &source, const QString &target, QStringList &log)
{
    if (QDir::cleanPath(source) == QDir::cleanPath(target))
        return true;
    QDir().mkpath(QFileInfo(target).absolutePath());
    QFile::remove(target);
    if (!QFile::rename(source, target)) {
        log << QCoreApplication::translate("Crowbar", "WARNING: Could not move \"%1\" to \"%2\".").arg(source, target);
        return false;
    }
    log << QCoreApplication::translate("Crowbar", "Moved \"%1\" to \"%2\".").arg(QFileInfo(source).fileName(), QFileInfo(target).absolutePath());
    return true;
}

} // namespace

QString toolName(Format format)
{
    return format == Format::Vpk ? "vpk" : "gmad";
}

QString extension(Format format)
{
    return format == Format::Vpk ? "vpk" : "gma";
}

launcher::Command packCommand(const steam::App &app, Format format, const QString &folder,
                              const QString &outputFile, const Options &options)
{
    launcher::Command command;
    const QString executable = launcher::findTool(app, toolName(format));
    if (executable.isEmpty()) {
        command.error = QCoreApplication::translate("Crowbar", "%1 has no %2 in its bin folder.").arg(app.name, toolName(format));
        return command;
    }
    const QFileInfo input(QDir::cleanPath(folder));
    if (!input.isDir()) {
        command.error = QCoreApplication::translate("Crowbar", "\"%1\" is not a folder.").arg(folder);
        return command;
    }
    if (format == Format::Gma && !hasAddonJson(input.absoluteFilePath())) {
        command.error = QCoreApplication::translate("Crowbar", "gmad needs an addon.json file in \"%1\".").arg(folder);
        return command;
    }

    const bool windows = launcher::isWindowsExecutable(executable);
    const auto path = [&](const QString &hostPath) {
        return windows ? launcher::toWindowsPath(hostPath) : QDir::cleanPath(hostPath);
    };

    QStringList arguments;
    if (format == Format::Vpk) {
        if (options.multiChunk)
            arguments << "-M";
        arguments += options.extra;
        arguments << path(input.absoluteFilePath());
    } else {
        QDir().mkpath(QFileInfo(outputFile).absolutePath());
        arguments << "create" << "-folder" << path(input.absoluteFilePath()) << "-out" << path(outputFile);
        if (options.warnInvalid)
            arguments << "-warninvalid";
        arguments += options.extra;
    }

    command = launcher::build(app, executable, arguments, launcher::Mode::Console);
    command.workingDirectory = input.absolutePath();
    return command;
}

QStringList finishPack(Format format, const QString &folder, const QString &outputFile,
                       const Options &options, const QDateTime &started, QStringList &log)
{
    if (format == Format::Gma) {
        if (QFile::exists(outputFile))
            return {outputFile};
        log << QCoreApplication::translate("Crowbar", "WARNING: The packer did not create \"%1\".").arg(outputFile);
        return {};
    }

    const QFileInfo input(QDir::cleanPath(folder));
    const QFileInfo output(outputFile);
    const QString sourceBase = input.absolutePath() + u'/' + input.fileName();
    const QString targetBase = output.absolutePath() + u'/' + output.completeBaseName();

    if (!options.multiChunk) {
        const QString packed = sourceBase + ".vpk";
        if (!QFile::exists(packed)) {
            log << QCoreApplication::translate("Crowbar", "WARNING: The packer did not create \"%1\".").arg(packed);
            return {};
        }
        return moveFile(packed, outputFile, log) ? QStringList{outputFile} : QStringList{};
    }

    static const QRegularExpression chunkSuffix("_(dir|\\d{3})\\.vpk$", QRegularExpression::CaseInsensitiveOption);
    QStringList result;
    const QDir parent(input.absolutePath());
    for (const QFileInfo &file : parent.entryInfoList({input.fileName() + "_*.vpk"}, QDir::Files, QDir::Name)) {
        const auto match = chunkSuffix.match(file.fileName());
        if (!match.hasMatch() || file.fileName().left(match.capturedStart()) != input.fileName())
            continue;
        if (file.lastModified() < started.addSecs(-2))
            continue;
        const QString target = targetBase + match.captured(0).toLower();
        if (moveFile(file.absoluteFilePath(), target, log))
            result << target;
    }
    if (result.isEmpty())
        log << QCoreApplication::translate("Crowbar", "WARNING: The packer did not create \"%1_dir.vpk\".").arg(sourceBase);
    return result;
}

QStringList addonTypes()
{
    return {"gamemode", "map", "weapon", "vehicle", "npc", "entity", "tool", "effects", "model", "servercontent"};
}

QStringList addonTags()
{
    return {"fun", "roleplay", "scenic", "movie", "realism", "cartoon", "water", "comic", "build"};
}

bool hasAddonJson(const QString &folder)
{
    return QFile::exists(folder + "/addon.json");
}

bool writeAddonJson(const QString &folder, const QString &title, const QString &type,
                    const QStringList &tags, QString *error)
{
    QJsonObject root;
    root["title"] = title;
    root["type"] = type;
    root["tags"] = QJsonArray::fromStringList(tags);
    root["ignore"] = QJsonArray::fromStringList({"*.psd", "*.vcproj", "*.svn*"});

    QFile file(folder + "/addon.json");
    // NewOnly: an existing addon.json is the user's and is never replaced.
    if (!file.open(QIODevice::WriteOnly | QIODevice::NewOnly)) {
        if (error)
            *error = file.errorString();
        return false;
    }
    file.write(QJsonDocument(root).toJson(QJsonDocument::Indented));
    return true;
}

} // namespace packer
