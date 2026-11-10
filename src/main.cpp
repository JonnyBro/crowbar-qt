#include "core/Steam.h"
#include "core/Studiomdl.h"
#include "core/ToolProcess.h"
#include "ui/MainWindow.h"

#include <QApplication>
#include <QCoreApplication>
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

int compile(QCoreApplication &app, quint32 appId, const QString &gameDir, const QString &qcFile)
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
        if (std::strcmp(argv[i], "--compile") == 0) {
            if (i + 3 >= argc) {
                QTextStream(stderr) << "Usage: crowbar-qt --compile <appid> <game dir> <qc file>\n";
                return 2;
            }
            QCoreApplication app(argc, argv);
            return compile(app, QString::fromLocal8Bit(argv[i + 1]).toUInt(),
                           QString::fromLocal8Bit(argv[i + 2]), QString::fromLocal8Bit(argv[i + 3]));
        }
    }

    QApplication app(argc, argv);
    MainWindow window;
    window.show();
    return app.exec();
}
