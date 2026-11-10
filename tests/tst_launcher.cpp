#include "core/Hlmv.h"
#include "core/Launcher.h"
#include "core/Runner.h"
#include "core/Studiomdl.h"
#include "core/ToolProcess.h"

#include <QDir>
#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

namespace {

void writeFile(const QString &path, const QByteArray &content = {})
{
    QDir().mkpath(QFileInfo(path).absolutePath());
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(content);
}

} // namespace

class TestLauncher : public QObject {
    Q_OBJECT

private slots:
    void windowsPaths()
    {
        QCOMPARE(launcher::toWindowsPath("/mnt/hdd/Steam Library/game"), "Z:\\mnt\\hdd\\Steam Library\\game");
        QCOMPARE(launcher::toWindowsPath("/a/b/../c/"), "Z:\\a\\c");
    }

    void hostPaths()
    {
        QCOMPARE(launcher::toHostPaths("writing Z:\\mnt\\game\\models/a/tri.mdl:"), "writing /mnt/game/models/a/tri.mdl:");
        QCOMPARE(launcher::toHostPaths("gamedir: \"z:\\mnt\\my game\\\""), "gamedir: \"/mnt/my game/\"");
        QCOMPARE(launcher::toHostPaths("Z:\\a\\b.smd (3) : bad\\token"), "/a/b.smd (3) : bad\\token");
        QCOMPARE(launcher::toHostPaths("no paths here"), "no paths here");
    }

    void protonNoise()
    {
        QVERIFY(launcher::isProtonNoise(u"ProtonFixes[1] WARN: Skipping fix execution."));
        QVERIFY(launcher::isProtonNoise(u"ntsync: up and running."));
        QVERIFY(!launcher::isProtonNoise(u"Building binary model files..."));
    }

    void buildCommands()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        const QString dir = temp.path();
        writeFile(dir + "/game/bin/studiomdl.exe", "MZ\x90");
        writeFile(dir + "/game/hl2/gameinfo.txt");
        writeFile(dir + "/src/model.qc");
        writeFile(dir + "/native/tool", "\x7f" "ELF");

        steam::App app;
        app.id = 4000;
        app.name = "Test";
        app.steamRoot = dir + "/steam";
        app.toolsPath = dir + "/game/bin";

        launcher::Command command = studiomdl::compileCommand(app, dir + "/game/hl2", dir + "/src/model.qc", {});
        QVERIFY(!command.isValid());
        QVERIFY(command.error.contains("Proton"));

        app.protonPath = dir + "/proton";
        app.compatDataPath = dir + "/compatdata/4000";
        qputenv("PROTON_ENABLE_WAYLAND", "1");
        studiomdl::Options options;
        options.verbose = true;
        options.extra = {"-nox360"};
        command = studiomdl::compileCommand(app, dir + "/game/hl2", dir + "/src/model.qc", options);
        QVERIFY(command.isValid());
        QCOMPARE(command.program, dir + "/proton/proton");
        QCOMPARE(command.arguments,
                 QStringList({"runinprefix", dir + "/game/bin/studiomdl.exe", "-game",
                              launcher::toWindowsPath(dir + "/game/hl2"), "-nop4", "-verbose", "-nox360", "model.qc"}));
        QCOMPARE(command.workingDirectory, dir + "/src");
        QCOMPARE(command.environment.value("STEAM_COMPAT_DATA_PATH"), app.compatDataPath);
        QCOMPARE(command.environment.value("SteamAppId"), "4000");
        QVERIFY(!command.environment.contains("PROTON_ENABLE_WAYLAND"));

        command = launcher::build(app, dir + "/game/bin/studiomdl.exe", {}, launcher::Mode::Gui);
        QCOMPARE(command.arguments.first(), "run");

        command = launcher::build(app, dir + "/native/tool", {"-a"}, launcher::Mode::Console);
        QCOMPARE(command.program, dir + "/native/tool");
        QCOMPARE(command.arguments, QStringList({"-a"}));

        QVERIFY(!launcher::build(app, dir + "/missing", {}, launcher::Mode::Console).isValid());
        QVERIFY(!studiomdl::compileCommand(app, dir + "/game/hl2", dir + "/src/missing.qc", {}).isValid());
    }

    void modelName()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        const QString qc = temp.path() + "/a.qc";

        writeFile(qc, "// $modelname \"wrong.mdl\"\n$ModelName \"Props\\\\My Model\\\\Crate.MDL\"\n$body b \"a.smd\"\n");
        QCOMPARE(studiomdl::readModelName(qc), "Props/My Model/Crate.MDL");
        writeFile(qc, "$cdmaterials x\n\t$modelname props/crate\n");
        QCOMPARE(studiomdl::readModelName(qc), "props/crate.mdl");
        writeFile(qc, "$body b \"a.smd\"\n");
        QCOMPARE(studiomdl::readModelName(qc), QString());
    }

    void moveCompiled()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        const QString game = temp.path() + "/game";
        const QString out = temp.path() + "/out";
        for (const char *name : {"crate.mdl", "crate.vvd", "crate.dx90.vtx", "crate.phy", "crate.txt", "crate2.mdl"})
            writeFile(game + "/models/Props/new/" + name, name);
        writeFile(out + "/models/props/new/crate.mdl", "old");

        QStringList log;
        QVERIFY(studiomdl::moveCompiledModel(game, "props/new/crate.mdl", out, log));
        const QDir target(out + "/models/props/new");
        QCOMPARE(target.entryList(QDir::Files), QStringList({"crate.dx90.vtx", "crate.mdl", "crate.phy", "crate.vvd"}));
        QFile moved(target.filePath("crate.mdl"));
        QVERIFY(moved.open(QIODevice::ReadOnly));
        QCOMPARE(moved.readAll(), "crate.mdl");
        QCOMPARE(QDir(game + "/models/Props/new").entryList(QDir::Files), QStringList({"crate.txt", "crate2.mdl"}));
        QCOMPARE(log.size(), 4);

        log.clear();
        writeFile(game + "/models/solo/dir/a.mdl");
        QVERIFY(studiomdl::moveCompiledModel(game, "solo/dir/a.mdl", out, log));
        QVERIFY(!QDir(game + "/models/solo").exists());
        QVERIFY(QDir(game + "/models").exists());

        writeFile(game + "/models/top.mdl");
        QVERIFY(studiomdl::moveCompiledModel(game, "top.mdl", out, log));
        QVERIFY(QFile::exists(out + "/models/top.mdl"));
        QVERIFY(QDir(game + "/models").exists());

        log.clear();
        QVERIFY(!studiomdl::moveCompiledModel(game, "missing/x.mdl", out, log));
        QVERIFY(!studiomdl::moveCompiledModel(game, "props/new/absent.mdl", out, log));
        QCOMPARE(log.size(), 2);
    }

    void processOutputAndExitCode()
    {
        launcher::Command command;
        command.program = "/bin/sh";
        command.arguments = {"-c", "echo 'ntsync: up and running.'; echo 'writing Z:\\tmp\\a.mdl'; printf 'tail'; exit 3"};
        command.environment = QProcessEnvironment::systemEnvironment();

        ToolProcess process;
        QSignalSpy lines(&process, &ToolProcess::outputLine);
        QSignalSpy finished(&process, &ToolProcess::finished);
        process.start(command);
        QVERIFY(finished.wait(5000));
        QCOMPARE(lines.size(), 2);
        QCOMPARE(lines[0][0].toString(), "writing /tmp/a.mdl");
        QCOMPARE(lines[1][0].toString(), "tail");
        QCOMPARE(finished[0][0].toInt(), 3);
        QCOMPARE(finished[0][1].toBool(), false);
    }

    void cancelKillsChildren()
    {
        launcher::Command command;
        command.program = "/bin/sh";
        command.arguments = {"-c", "setsid sleep 31.7 & sleep 30 & sleep 30"};
        command.environment = QProcessEnvironment::systemEnvironment();

        ToolProcess process;
        QSignalSpy finished(&process, &ToolProcess::finished);
        process.start(command);
        QTRY_VERIFY(process.isRunning());
        QTest::qWait(100);
        process.cancel();
        QVERIFY(finished.wait(5000));
        QCOMPARE(finished[0][1].toBool(), false);

        // The setsid child left our process group, like Windows programs under Wine do.
        QTRY_COMPARE(QProcess::execute("pgrep", {"-f", "^sleep 31\\.7$"}), 1);
    }

    void hlmvCommand()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        const QString dir = temp.path();
        writeFile(dir + "/game/bin/hlmv.exe", "MZ");
        writeFile(dir + "/models/a.mdl");

        steam::App app;
        app.id = 4000;
        app.toolsPath = dir + "/game/bin";
        app.protonPath = dir + "/proton";
        app.compatDataPath = dir + "/compatdata";

        launcher::Command command = hlmv::viewCommand(app, dir + "/game/hl2", dir + "/models/a.mdl");
        QVERIFY(command.isValid());
        QCOMPARE(command.arguments,
                 QStringList({"run", dir + "/game/bin/hlmv.exe", "-olddialogs", "-game",
                              launcher::toWindowsPath(dir + "/game/hl2"), launcher::toWindowsPath(dir + "/models/a.mdl")}));

        command = hlmv::viewCommand(app, dir + "/game/hl2", {});
        QCOMPARE(command.arguments.size(), 5);
        QVERIFY(!hlmv::viewCommand(app, dir + "/game/hl2", dir + "/missing.mdl").isValid());
    }

    void runCommands()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        const QString dir = temp.path();
        writeFile(dir + "/game/bin/hammer.exe", "MZ");

        steam::App app;
        app.id = 4000;
        app.installPath = dir + "/game";
        app.steamRoot = dir + "/steam";
        app.toolsPath = dir + "/game/bin";
        app.protonPath = dir + "/proton";
        app.compatDataPath = dir + "/compatdata";

        launcher::Command command = runner::hammerCommand(app, dir + "/game/garrysmod");
        QCOMPARE(command.arguments, QStringList({"run", dir + "/game/bin/hammer.exe", "-nop4", "-game",
                                                 launcher::toWindowsPath(dir + "/game/garrysmod")}));

        command = runner::gameCommand(app, dir + "/game/garrysmod", {"-dev", "+map", "gm_construct"});
        if (command.isValid())
            QCOMPARE(command.arguments, QStringList({"-applaunch", "4000", "-game", "garrysmod", "-dev", "+map", "gm_construct"}));

        app.steamRoot = QDir::homePath() + "/.var/app/com.valvesoftware.Steam/.local/share/Steam";
        command = runner::gameCommand(app, dir + "/game/garrysmod", {});
        if (command.isValid())
            QCOMPARE(command.arguments.mid(0, 4), QStringList({"run", "com.valvesoftware.Steam", "-applaunch", "4000"}));

        app.toolsPath.clear();
        QVERIFY(!runner::hammerCommand(app, dir + "/game/garrysmod").isValid());
    }

    void failedToStart()
    {
        launcher::Command command;
        command.program = "/nonexistent/program";
        ToolProcess process;
        QSignalSpy failed(&process, &ToolProcess::failedToStart);
        process.start(command);
        QVERIFY(failed.wait(5000));
    }
};

QTEST_GUILESS_MAIN(TestLauncher)
#include "tst_launcher.moc"
