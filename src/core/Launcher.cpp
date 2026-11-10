#include "core/Launcher.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QProcess>
#include <QRegularExpression>

namespace launcher {

QString Command::displayString() const
{
    QStringList parts{program};
    parts += arguments;
    for (QString &part : parts) {
        if (part.isEmpty() || part.contains(u' '))
            part = u'"' + part + u'"';
    }
    return parts.join(u' ');
}

bool isWindowsExecutable(const QString &path)
{
    QFile file(path);
    return file.open(QIODevice::ReadOnly) && file.read(2) == "MZ";
}

QString findTool(const steam::App &app, const QString &name)
{
    if (app.toolsPath.isEmpty())
        return {};
    for (const QString &fileName : {name + ".exe", name}) {
        const QString path = app.toolsPath + '/' + fileName;
        if (QFile::exists(path))
            return path;
    }
    return {};
}

QString toWindowsPath(const QString &hostPath)
{
    QString path = QDir::cleanPath(QFileInfo(hostPath).absoluteFilePath());
    return "Z:" + path.replace(u'/', u'\\');
}

QString toHostPaths(QString text)
{
    static const QRegularExpression pattern(R"([Zz]:[\\/][^"\r\n:*?<>|]*)");
    QList<QRegularExpressionMatch> matches;
    auto it = pattern.globalMatch(text);
    while (it.hasNext())
        matches << it.next();
    for (auto match = matches.crbegin(); match != matches.crend(); ++match) {
        QString path = match->captured().mid(2);
        path.replace(u'\\', u'/');
        text.replace(match->capturedStart(), match->capturedLength(), path);
    }
    return text;
}

bool isProtonNoise(QStringView line)
{
    return line.startsWith(u"ProtonFixes[") || line.startsWith(u"ntsync:")
        || line.startsWith(u"fsync:") || line.startsWith(u"esync:");
}

Command build(const steam::App &app, const QString &executable, const QStringList &arguments, Mode mode)
{
    Command command;
    command.environment = QProcessEnvironment::systemEnvironment();

    if (!QFile::exists(executable)) {
        command.error = QStringLiteral("\"%1\" does not exist.").arg(executable);
        return command;
    }

    if (!isWindowsExecutable(executable)) {
        command.program = executable;
        command.arguments = arguments;
        return command;
    }

    if (app.protonPath.isEmpty()) {
        command.error = QStringLiteral("No Proton version found for %1.").arg(app.name);
        return command;
    }
    if (app.compatDataPath.isEmpty()) {
        command.error = QStringLiteral("%1 has no Proton prefix yet. Start the game once from Steam.").arg(app.name);
        return command;
    }

    // "run" goes through Proton's steam.exe shim, which gives the program a
    // working Steam API but drops its stdout. "runinprefix" skips the shim.
    command.program = app.protonPath + "/proton";
    command.arguments = {mode == Mode::Console ? "runinprefix" : "run", executable};
    command.arguments += arguments;

    const QString appId = QString::number(app.id);
    command.environment.insert("STEAM_COMPAT_DATA_PATH", app.compatDataPath);
    command.environment.insert("STEAM_COMPAT_CLIENT_INSTALL_PATH", app.steamRoot);
    command.environment.insert("SteamAppId", appId);
    command.environment.insert("SteamGameId", appId);
    // Under Wine's Wayland driver the menus of these old Win32 tools close as
    // soon as they open, so a global PROTON_ENABLE_WAYLAND=1 must not reach them.
    command.environment.remove("PROTON_ENABLE_WAYLAND");
    return command;
}

} // namespace launcher
