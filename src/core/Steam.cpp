#include "core/Steam.h"

#include "core/Vdf.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QRegularExpression>
#include <QSet>

#include <algorithm>

namespace steam {

namespace {

QString canonical(const QString &path)
{
    return QFileInfo(path).canonicalFilePath();
}

const vdf::Node *descend(const vdf::Node &root, std::initializer_list<QStringView> path)
{
    const vdf::Node *node = &root;
    for (QStringView key : path) {
        node = node->find(key);
        if (!node)
            return nullptr;
    }
    return node;
}

QStringList subdirectories(const QString &path)
{
    QStringList result;
    const QDir dir(path);
    for (const QString &name : dir.entryList(QDir::Dirs | QDir::NoDotAndDotDot))
        result << dir.absoluteFilePath(name);
    return result;
}

bool hasFile(const QString &directory, const QStringList &patterns)
{
    return !QDir(directory).entryList(patterns, QDir::Files).isEmpty();
}

QStringList readLibraries(const QString &root)
{
    QStringList result;
    QSet<QString> seen;
    const auto add = [&](const QString &path) {
        const QString resolved = canonical(path);
        if (resolved.isEmpty() || seen.contains(resolved) || !QDir(resolved + "/steamapps").exists())
            return;
        seen.insert(resolved);
        result << resolved;
    };

    add(root);
    for (const QString &file : {root + "/steamapps/libraryfolders.vdf", root + "/config/libraryfolders.vdf"}) {
        const auto document = vdf::parseFile(file);
        const vdf::Node *folders = document ? document->find(u"libraryfolders") : nullptr;
        if (!folders)
            continue;
        for (const vdf::Node &entry : folders->children) {
            if (entry.isObject) {
                add(entry.get(u"path"));
            } else {
                // Pre-2021 format: "1" "/path/to/library" next to unrelated string keys.
                bool numeric = false;
                entry.key.toUInt(&numeric);
                if (numeric)
                    add(entry.value);
            }
        }
    }
    return result;
}

QList<CompatTool> readCompatTools(const QString &root, const QStringList &libraries)
{
    QList<CompatTool> tools;

    QStringList customDirs = {
        root + "/compatibilitytools.d",
        "/usr/share/steam/compatibilitytools.d",
        "/usr/local/share/steam/compatibilitytools.d",
    };
    customDirs += qEnvironmentVariable("STEAM_EXTRA_COMPAT_TOOLS_PATHS").split(u':', Qt::SkipEmptyParts);

    for (const QString &customDir : customDirs) {
        for (const QString &toolDir : subdirectories(customDir)) {
            const auto document = vdf::parseFile(toolDir + "/compatibilitytool.vdf");
            const vdf::Node *entries = document ? descend(*document, {u"compatibilitytools", u"compat_tools"}) : nullptr;
            if (!entries)
                continue;
            for (const vdf::Node &entry : entries->children) {
                const QString path = QDir::cleanPath(QDir(toolDir).absoluteFilePath(entry.get(u"install_path", ".")));
                if (QFile::exists(path + "/proton"))
                    tools << CompatTool{entry.key, entry.get(u"display_name", entry.key), path};
            }
        }
    }

    for (const QString &library : libraries) {
        for (const QString &toolDir : subdirectories(library + "/steamapps/common")) {
            const QString directoryName = QFileInfo(toolDir).fileName();
            const QString name = officialToolName(directoryName);
            if (!name.isEmpty() && QFile::exists(toolDir + "/proton"))
                tools << CompatTool{name, directoryName, toolDir};
        }
    }
    return tools;
}

QHash<quint32, QString> readCompatMapping(const QString &root)
{
    QHash<quint32, QString> mapping;
    const auto document = vdf::parseFile(root + "/config/config.vdf");
    const vdf::Node *entries = document
        ? descend(*document, {u"InstallConfigStore", u"Software", u"Valve", u"Steam", u"CompatToolMapping"})
        : nullptr;
    if (!entries)
        return mapping;
    for (const vdf::Node &entry : entries->children) {
        const QString name = entry.get(u"name");
        if (!name.isEmpty())
            mapping.insert(entry.key.toUInt(), name);
    }
    return mapping;
}

// config_info is written by Proton into the prefix and lists paths inside the
// Proton build that last used it; that is the only record when the tool was
// picked by Steam's global default rather than a per-app mapping.
QString protonFromConfigInfo(const QString &compatDataPath)
{
    QFile file(compatDataPath + "/config_info");
    if (!file.open(QIODevice::ReadOnly))
        return {};
    const QStringList lines = QString::fromUtf8(file.readAll()).split(u'\n', Qt::SkipEmptyParts);
    for (const QString &line : lines) {
        for (const QString &marker : {QStringLiteral("/files/"), QStringLiteral("/dist/")}) {
            const qsizetype index = line.indexOf(marker);
            if (index > 0 && QFile::exists(line.left(index) + "/proton"))
                return line.left(index);
        }
    }
    return {};
}

Platform detectPlatform(const vdf::Node &appState, const QString &installPath, bool hasPrefix)
{
    if (const vdf::Node *userConfig = appState.find(u"UserConfig")) {
        if (userConfig->get(u"platform_override_source").compare(u"windows", Qt::CaseInsensitive) == 0)
            return Platform::Windows;
    }
    if (hasFile(installPath, {"*.sh", "*_linux", "*_linux64", "*.x86_64"}))
        return Platform::Linux;
    if (hasFile(installPath, {"*.exe"}))
        return Platform::Windows;
    return hasPrefix ? Platform::Windows : Platform::Linux;
}

QString findToolsPath(const QString &installPath)
{
    for (const char *sub : {"bin", "bin/x64", "bin/win64", "bin/linux64"}) {
        const QString path = installPath + '/' + sub;
        if (hasFile(path, {"studiomdl.exe", "studiomdl"}))
            return path;
    }
    return {};
}

QString findCompatData(const QStringList &libraries, quint32 appId)
{
    for (const QString &library : libraries) {
        const QString path = QStringLiteral("%1/steamapps/compatdata/%2").arg(library).arg(appId);
        if (QDir(path + "/pfx").exists())
            return path;
    }
    return {};
}

void resolveProton(App &app, const QHash<quint32, QString> &mapping, const QList<CompatTool> &tools)
{
    if (app.platform != Platform::Windows && !mapping.contains(app.id))
        return;

    QString name = mapping.value(app.id);
    if (name.isEmpty())
        name = mapping.value(0);

    const auto tool = std::find_if(tools.begin(), tools.end(), [&](const CompatTool &candidate) {
        return candidate.name.compare(name, Qt::CaseInsensitive) == 0;
    });
    if (tool != tools.end()) {
        app.compatToolName = tool->name;
        app.protonPath = tool->path;
        return;
    }

    app.compatToolName = name;
    if (app.compatDataPath.isEmpty())
        return;
    app.protonPath = protonFromConfigInfo(app.compatDataPath);
    if (!app.protonPath.isEmpty() && app.compatToolName.isEmpty())
        app.compatToolName = QFileInfo(app.protonPath).fileName();
}

} // namespace

QString officialToolName(const QString &directoryName)
{
    if (directoryName == "Proton - Experimental")
        return "proton_experimental";
    if (directoryName == "Proton Hotfix")
        return "proton_hotfix";

    static const QRegularExpression version("^Proton (\\d+)\\.(\\d+)");
    const auto match = version.match(directoryName);
    if (!match.hasMatch())
        return {};
    // Steam switched from "proton_63"-style names to major-only names with Proton 7.
    if (match.captured(1).toInt() >= 7)
        return "proton_" + match.captured(1);
    return "proton_" + match.captured(1) + match.captured(2);
}

void overrideTools(App &app, const QString &toolsPath, const QList<CompatTool> &tools, const QString &prefixDir)
{
    if (!QDir(toolsPath).exists())
        return;
    app.toolsPath = QDir::cleanPath(toolsPath);
    app.toolsOverridden = true;
    if (!app.protonPath.isEmpty() || QDir(toolsPath).entryList({"*.exe"}, QDir::Files).isEmpty())
        return;

    // Valve's own builds first; otherwise the last custom one found.
    const CompatTool *chosen = nullptr;
    for (const char *preferred : {"proton_experimental", "proton_hotfix"}) {
        for (const CompatTool &tool : tools) {
            if (!chosen && tool.name == QLatin1String(preferred))
                chosen = &tool;
        }
    }
    for (const CompatTool &tool : tools) {
        if (!chosen && tool.name.startsWith("proton_"))
            chosen = &tool;
    }
    if (!chosen && !tools.isEmpty())
        chosen = &tools.last();
    if (!chosen)
        return;

    app.protonPath = chosen->path;
    app.compatToolName = chosen->name;
    app.compatDataPath = QDir::cleanPath(prefixDir);
    QDir().mkpath(app.compatDataPath);
}

QStringList findRoots()
{
    const QString home = QDir::homePath();
    const QStringList candidates = {
        home + "/.local/share/Steam",
        home + "/.steam/steam",
        home + "/.steam/root",
        home + "/.steam/debian-installation",
        home + "/.var/app/com.valvesoftware.Steam/.local/share/Steam",
        home + "/.var/app/com.valvesoftware.Steam/data/Steam",
        home + "/snap/steam/common/.local/share/Steam",
    };

    QStringList roots;
    for (const QString &candidate : candidates) {
        const QString resolved = canonical(candidate);
        if (!resolved.isEmpty() && !roots.contains(resolved) && QDir(resolved + "/steamapps").exists())
            roots << resolved;
    }
    return roots;
}

Installation scan(const QString &root)
{
    Installation installation;
    installation.root = root;
    installation.libraries = readLibraries(root);
    installation.compatTools = readCompatTools(root, installation.libraries);
    const QHash<quint32, QString> mapping = readCompatMapping(root);

    for (const QString &library : installation.libraries) {
        const QDir steamapps(library + "/steamapps");
        for (const QString &manifest : steamapps.entryList({"appmanifest_*.acf"}, QDir::Files)) {
            const auto document = vdf::parseFile(steamapps.absoluteFilePath(manifest));
            const vdf::Node *state = document ? document->find(u"AppState") : nullptr;
            if (!state)
                continue;

            App app;
            app.id = state->get(u"appid").toUInt();
            app.name = state->get(u"name");
            app.steamRoot = root;
            app.installPath = steamapps.absoluteFilePath("common/" + state->get(u"installdir"));
            if (app.id == 0 || !QDir(app.installPath).exists())
                continue;

            for (const QString &directory : subdirectories(app.installPath)) {
                if (hasFile(directory, {"gameinfo.txt"}))
                    app.gameDirs << directory;
            }
            app.toolsPath = findToolsPath(app.installPath);
            app.compatDataPath = findCompatData(installation.libraries, app.id);
            app.platform = detectPlatform(*state, app.installPath, !app.compatDataPath.isEmpty());
            resolveProton(app, mapping, installation.compatTools);
            installation.apps << app;
        }
    }

    std::sort(installation.apps.begin(), installation.apps.end(), [](const App &a, const App &b) {
        return a.name.localeAwareCompare(b.name) < 0;
    });
    return installation;
}

QList<Installation> scanAll()
{
    QList<Installation> installations;
    for (const QString &root : findRoots())
        installations << scan(root);
    return installations;
}

} // namespace steam
