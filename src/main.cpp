#include "core/Hlmv.h"
#include "core/Package.h"
#include "core/Packer.h"
#include "core/Steam.h"
#include "core/Studiomdl.h"
#include "core/ToolProcess.h"
#include "ui/MainWindow.h"

#include <QApplication>
#include <QCoreApplication>
#include <QFileInfo>
#include <QTextStream>

#include <cstring>

namespace {

int listGames()
{
    QTextStream out(stdout);
    for (const steam::Installation &installation : steam::scanAll()) {
        out << "Steam: " << installation.root << '\n';
        for (const QString &library : installation.libraries)
            out << "  library: " << library << '\n';
        for (const steam::CompatTool &tool : installation.compatTools)
            out << "  compat tool: " << tool.name << " -> " << tool.path << '\n';
        for (const steam::App &app : installation.apps) {
            if (!app.isSourceGame())
                continue;
            out << '\n' << app.name << " (" << app.id << ")\n";
            out << "  build:   " << (app.platform == steam::Platform::Windows ? "windows" : "linux") << '\n';
            out << "  install: " << app.installPath << '\n';
            out << "  tools:   " << (app.toolsPath.isEmpty() ? "-" : app.toolsPath) << '\n';
            if (app.platform == steam::Platform::Windows) {
                out << "  proton:  " << (app.protonPath.isEmpty() ? "not found" : app.protonPath)
                    << " [" << app.compatToolName << "]\n";
                out << "  prefix:  " << (app.compatDataPath.isEmpty() ? "-" : app.compatDataPath) << '\n';
            }
            for (const QString &gameDir : app.gameDirs)
                out << "  game:    " << gameDir << '\n';
        }
    }
    return 0;
}

int run(QCoreApplication &app, const launcher::Command &command)
{
    QTextStream out(stdout);
    QTextStream err(stderr);
    if (!command.isValid()) {
        err << command.error << Qt::endl;
        return 1;
    }
    ToolProcess process;
    QObject::connect(&process, &ToolProcess::outputLine, [&](const QString &line) { out << line << Qt::endl; });
    QObject::connect(&process, &ToolProcess::failedToStart, [&](const QString &error) {
        err << error << Qt::endl;
        app.exit(1);
    });
    QObject::connect(&process, &ToolProcess::finished, [&](int exitCode, bool success) {
        app.exit(success ? 0 : (exitCode ? exitCode : 1));
    });
    process.start(command);
    return app.exec();
}

std::optional<steam::App> findApp(quint32 appId)
{
    for (const steam::Installation &installation : steam::scanAll()) {
        for (const steam::App &game : installation.apps) {
            if (game.id == appId)
                return game;
        }
    }
    QTextStream(stderr) << "App " << appId << " is not installed." << Qt::endl;
    return std::nullopt;
}

int listPackage(const QString &file)
{
    QString error;
    const auto package = package::Package::open(file, &error);
    if (!package) {
        QTextStream(stderr) << error << Qt::endl;
        return 1;
    }
    QTextStream out(stdout);
    for (const package::Entry &entry : package->entries())
        out << entry.size << '\t' << entry.path << '\n';
    return 0;
}

int unpack(const QString &file, const QString &outputDir)
{
    QTextStream err(stderr);
    QString error;
    const auto package = package::Package::open(file, &error);
    if (!package) {
        err << error << Qt::endl;
        return 1;
    }
    int failed = 0;
    for (const package::Entry &entry : package->entries()) {
        if (!package->extract(entry, outputDir, &error)) {
            err << error << Qt::endl;
            ++failed;
        }
    }
    QTextStream(stdout) << package->format() << ": " << package->entries().size() - failed << " file(s) unpacked, "
                        << failed << " failed" << Qt::endl;
    return failed ? 1 : 0;
}

int pack(QCoreApplication &app, quint32 appId, const QString &formatName, const QString &folder,
         const QString &outputFile, const QStringList &flags)
{
    const auto game = findApp(appId);
    if (!game)
        return 1;
    const auto format = formatName == "gma" ? packer::Format::Gma : packer::Format::Vpk;
    const QString output = QFileInfo(outputFile).absoluteFilePath();
    packer::Options options;
    for (const QString &flag : flags) {
        if (flag == "--multi")
            options.multiChunk = true;
        else if (flag == "-warninvalid")
            options.warnInvalid = true;
        else
            options.extra << flag;
    }
    const QDateTime started = QDateTime::currentDateTime();
    const int result = run(app, packer::packCommand(*game, format, folder, output, options));
    if (result != 0)
        return result;
    QStringList log;
    const QStringList files = packer::finishPack(format, folder, output, options, started, log);
    for (const QString &line : log)
        QTextStream(stdout) << line << Qt::endl;
    return files.isEmpty() ? 1 : 0;
}

int view(QCoreApplication &app, quint32 appId, const QString &gameDir, const QString &mdlFile)
{
    for (const steam::Installation &installation : steam::scanAll()) {
        for (const steam::App &game : installation.apps) {
            if (game.id == appId)
                return run(app, hlmv::viewCommand(game, gameDir, mdlFile));
        }
    }
    QTextStream(stderr) << "App " << appId << " is not installed." << Qt::endl;
    return 1;
}

int compile(QCoreApplication &app, quint32 appId, const QString &gameDir, const QString &qcFile,
            const QString &outputDir)
{
    QTextStream out(stdout);
    QTextStream err(stderr);
    for (const steam::Installation &installation : steam::scanAll()) {
        for (const steam::App &game : installation.apps) {
            if (game.id != appId)
                continue;
            ToolProcess process;
            QObject::connect(&process, &ToolProcess::outputLine, [&](const QString &line) { out << line << Qt::endl; });
            QObject::connect(&process, &ToolProcess::failedToStart, [&](const QString &error) {
                err << error << Qt::endl;
                app.exit(1);
            });
            QObject::connect(&process, &ToolProcess::finished, [&](int exitCode, bool success) {
                if (success && !outputDir.isEmpty()) {
                    QStringList log;
                    success = studiomdl::moveCompiledModel(gameDir, studiomdl::readModelName(qcFile), outputDir, log);
                    for (const QString &line : log)
                        out << line << Qt::endl;
                }
                app.exit(success ? 0 : (exitCode ? exitCode : 1));
            });
            const launcher::Command command = studiomdl::compileCommand(game, gameDir, qcFile, {});
            if (!command.isValid()) {
                err << command.error << Qt::endl;
                return 1;
            }
            process.start(command);
            return app.exec();
        }
    }
    err << "App " << appId << " is not installed." << Qt::endl;
    return 1;
}

} // namespace

int main(int argc, char *argv[])
{
    QCoreApplication::setApplicationName("crowbar-qt");
    QCoreApplication::setOrganizationName("crowbar-qt");
    QCoreApplication::setApplicationVersion(CROWBAR_VERSION);

    // Checked before QApplication exists so the listing works without a display.
    for (int i = 1; i < argc; ++i) {
        if (std::strcmp(argv[i], "--list-games") == 0) {
            QCoreApplication app(argc, argv);
            return listGames();
        }
        if (std::strcmp(argv[i], "--list-package") == 0 && i + 1 < argc) {
            QCoreApplication app(argc, argv);
            return listPackage(QString::fromLocal8Bit(argv[i + 1]));
        }
        if (std::strcmp(argv[i], "--unpack") == 0 && i + 2 < argc) {
            QCoreApplication app(argc, argv);
            return unpack(QString::fromLocal8Bit(argv[i + 1]), QString::fromLocal8Bit(argv[i + 2]));
        }
        if (std::strcmp(argv[i], "--pack") == 0) {
            if (i + 4 >= argc) {
                QTextStream(stderr) << "Usage: crowbar-qt --pack <appid> <vpk|gma> <folder> <output file> [--multi] [packer options]\n";
                return 2;
            }
            QCoreApplication app(argc, argv);
            return pack(app, QString::fromLocal8Bit(argv[i + 1]).toUInt(), QString::fromLocal8Bit(argv[i + 2]),
                        QString::fromLocal8Bit(argv[i + 3]), QString::fromLocal8Bit(argv[i + 4]),
                        QCoreApplication::arguments().mid(i + 5));
        }
        if (std::strcmp(argv[i], "--view") == 0) {
            if (i + 2 >= argc) {
                QTextStream(stderr) << "Usage: crowbar-qt --view <appid> <game dir> [mdl file]\n";
                return 2;
            }
            QCoreApplication app(argc, argv);
            return view(app, QString::fromLocal8Bit(argv[i + 1]).toUInt(), QString::fromLocal8Bit(argv[i + 2]),
                        i + 3 < argc ? QString::fromLocal8Bit(argv[i + 3]) : QString());
        }
        if (std::strcmp(argv[i], "--compile") == 0) {
            if (i + 3 >= argc) {
                QTextStream(stderr) << "Usage: crowbar-qt --compile <appid> <game dir> <qc file> [output dir]\n";
                return 2;
            }
            QCoreApplication app(argc, argv);
            return compile(app, QString::fromLocal8Bit(argv[i + 1]).toUInt(),
                           QString::fromLocal8Bit(argv[i + 2]), QString::fromLocal8Bit(argv[i + 3]),
                           i + 4 < argc ? QString::fromLocal8Bit(argv[i + 4]) : QString());
        }
    }

    QApplication app(argc, argv);
    MainWindow window;
    window.show();
    return app.exec();
}
