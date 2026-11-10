#include "core/Studiomdl.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QRegularExpression>

#include <algorithm>

namespace studiomdl {

QString findExecutable(const steam::App &app)
{
    if (app.toolsPath.isEmpty())
        return {};
    for (const char *name : {"studiomdl.exe", "studiomdl"}) {
        const QString path = app.toolsPath + '/' + name;
        if (QFile::exists(path))
            return path;
    }
    return {};
}

launcher::Command compileCommand(const steam::App &app, const QString &gameDir,
                                 const QString &qcFile, const Options &options)
{
    const QString executable = findExecutable(app);
    if (executable.isEmpty()) {
        launcher::Command command;
        command.error = QStringLiteral("%1 has no studiomdl in its bin folder.").arg(app.name);
        return command;
    }

    const QFileInfo qc(qcFile);
    if (!qc.isFile()) {
        launcher::Command command;
        command.error = QStringLiteral("\"%1\" does not exist.").arg(qcFile);
        return command;
    }

    const bool windows = launcher::isWindowsExecutable(executable);
    QStringList arguments{"-game", windows ? launcher::toWindowsPath(gameDir) : gameDir};
    if (options.noP4)
        arguments << "-nop4";
    if (options.verbose)
        arguments << "-verbose";
    arguments += options.extra;
    // Relative name plus working directory, as studiomdl resolves $include and SMD paths from there.
    arguments << qc.fileName();

    launcher::Command command = launcher::build(app, executable, arguments, launcher::Mode::Console);
    command.workingDirectory = qc.absolutePath();
    return command;
}

QString readModelName(const QString &qcFile)
{
    QFile file(qcFile);
    if (!file.open(QIODevice::ReadOnly))
        return {};

    static const QRegularExpression pattern(
        R"(^\s*\$modelname\s+(?:"([^"]*)\"|(\S+)))",
        QRegularExpression::CaseInsensitiveOption | QRegularExpression::MultilineOption);
    const auto match = pattern.match(QString::fromUtf8(file.readAll()));
    if (!match.hasMatch())
        return {};

    QString name = match.captured(1).isEmpty() ? match.captured(2) : match.captured(1);
    name.replace(u'\\', u'/');
    name = QDir::cleanPath(name);
    if (!name.endsWith(".mdl", Qt::CaseInsensitive))
        name += ".mdl";
    return name;
}

namespace {

// The tools run under Wine, which matches existing folders ignoring case, so
// the path on disk may differ in case from what the QC says.
QString resolveIgnoringCase(const QString &base, const QString &relative)
{
    QString current = base;
    for (const QString &part : relative.split(u'/', Qt::SkipEmptyParts)) {
        const QDir dir(current);
        if (dir.exists(part)) {
            current = dir.filePath(part);
            continue;
        }
        const QStringList entries = dir.entryList(QDir::Dirs | QDir::NoDotAndDotDot);
        const auto found = std::find_if(entries.begin(), entries.end(), [&](const QString &entry) {
            return entry.compare(part, Qt::CaseInsensitive) == 0;
        });
        if (found == entries.end())
            return {};
        current = dir.filePath(*found);
    }
    return current;
}

} // namespace

bool moveCompiledModel(const QString &gameDir, const QString &modelName,
                       const QString &outputDir, QStringList &log)
{
    const QFileInfo model(modelName);
    const QString subpath = model.path() == "." ? QString() : model.path();
    const QString sourceDir = resolveIgnoringCase(gameDir, "models/" + subpath);
    if (sourceDir.isEmpty()) {
        log << QStringLiteral("WARNING: Compiled model folder \"%1/models/%2\" was not found.").arg(gameDir, subpath);
        return false;
    }

    const QString targetDir = QDir::cleanPath(outputDir + "/models/" + subpath);
    if (QFileInfo(targetDir) == QFileInfo(sourceDir))
        return true;
    if (!QDir().mkpath(targetDir)) {
        log << QStringLiteral("WARNING: Could not create \"%1\".").arg(targetDir);
        return false;
    }

    static const QStringList extensions{"ani", "mdl", "phy", "vtx", "vvd"};
    const QString prefix = model.completeBaseName() + '.';
    int moved = 0;
    for (const QFileInfo &file : QDir(sourceDir).entryInfoList(QDir::Files)) {
        if (!file.fileName().startsWith(prefix, Qt::CaseInsensitive)
            || !extensions.contains(file.suffix().toLower()))
            continue;
        const QString target = targetDir + '/' + file.fileName();
        QFile::remove(target);
        if (QFile::rename(file.absoluteFilePath(), target)) {
            ++moved;
            log << QStringLiteral("Moved \"%1\" to \"%2\".").arg(file.fileName(), targetDir);
        } else {
            log << QStringLiteral("WARNING: Could not move \"%1\" to \"%2\".").arg(file.absoluteFilePath(), targetDir);
        }
    }

    if (moved == 0)
        log << QStringLiteral("WARNING: No compiled files for \"%1\" found in \"%2\".").arg(modelName, sourceDir);
    // Only removes folders the compile created and the move just emptied.
    QDir(gameDir + "/models").rmpath(QDir(gameDir + "/models").relativeFilePath(sourceDir));
    return moved > 0;
}

} // namespace studiomdl
