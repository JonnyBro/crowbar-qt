#include "core/Launcher.h"
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

        command = launcher::build(app, dir + "/game/bin/studiomdl.exe", {}, launcher::Mode::Gui);
        QCOMPARE(command.arguments.first(), "run");

        command = launcher::build(app, dir + "/native/tool", {"-a"}, launcher::Mode::Console);
        QCOMPARE(command.program, dir + "/native/tool");
        QCOMPARE(command.arguments, QStringList({"-a"}));

        QVERIFY(!launcher::build(app, dir + "/missing", {}, launcher::Mode::Console).isValid());
        QVERIFY(!studiomdl::compileCommand(app, dir + "/game/hl2", dir + "/src/missing.qc", {}).isValid());
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
        command.arguments = {"-c", "sleep 30 & sleep 30"};
        command.environment = QProcessEnvironment::systemEnvironment();

        ToolProcess process;
        QSignalSpy finished(&process, &ToolProcess::finished);
        process.start(command);
        QTRY_VERIFY(process.isRunning());
        QTest::qWait(100);
        process.cancel();
        QVERIFY(finished.wait(5000));
        QCOMPARE(finished[0][1].toBool(), false);
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
