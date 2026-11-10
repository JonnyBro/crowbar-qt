#include "core/Steam.h"

#include <QDir>
#include <QFile>
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

const steam::App *findApp(const steam::Installation &installation, quint32 id)
{
    for (const steam::App &app : installation.apps) {
        if (app.id == id)
            return &app;
    }
    return nullptr;
}

} // namespace

class TestSteam : public QObject {
    Q_OBJECT

private slots:
    void officialToolNames()
    {
        QCOMPARE(steam::officialToolName("Proton - Experimental"), "proton_experimental");
        QCOMPARE(steam::officialToolName("Proton Hotfix"), "proton_hotfix");
        QCOMPARE(steam::officialToolName("Proton 9.0 (Beta)"), "proton_9");
        QCOMPARE(steam::officialToolName("Proton 6.3"), "proton_63");
        QCOMPARE(steam::officialToolName("Proton 5.13"), "proton_513");
        QCOMPARE(steam::officialToolName("GarrysMod"), QString());
    }

    void scanFindsGamesAndProton()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        const QString root = QDir(temp.path()).canonicalPath() + "/Steam";
        const QString library = QDir(temp.path()).canonicalPath() + "/Library";

        writeFile(root + "/steamapps/libraryfolders.vdf",
                  QStringLiteral("\"libraryfolders\"\n{\n\"0\"\n{\n\"path\" \"%1\"\n}\n\"1\"\n{\n\"path\" \"%2\"\n}\n"
                                 "\"2\"\n{\n\"path\" \"/nonexistent/library\"\n}\n}\n")
                      .arg(root, library).toUtf8());
        writeFile(root + "/config/config.vdf",
                  "\"InstallConfigStore\"\n{\n\"Software\"\n{\n\"valve\"\n{\n\"Steam\"\n{\n\"CompatToolMapping\"\n{\n"
                  "\"4000\"\n{\n\"name\" \"GE-Test\"\n\"config\" \"\"\n}\n}\n}\n}\n}\n}\n");
        writeFile(root + "/compatibilitytools.d/GE-Test/compatibilitytool.vdf",
                  "\"compatibilitytools\"\n{\n\"compat_tools\"\n{\n\"GE-Test\" // Internal name\n{\n"
                  "\"install_path\" \".\"\n\"display_name\" \"GE Test\"\n}\n}\n}\n");
        writeFile(root + "/compatibilitytools.d/GE-Test/proton");

        writeFile(library + "/steamapps/appmanifest_4000.acf",
                  "\"AppState\"\n{\n\"appid\" \"4000\"\n\"name\" \"Garry's Mod\"\n\"installdir\" \"GarrysMod\"\n}\n");
        writeFile(library + "/steamapps/common/GarrysMod/gmod.exe");
        writeFile(library + "/steamapps/common/GarrysMod/bin/studiomdl.exe");
        writeFile(library + "/steamapps/common/GarrysMod/garrysmod/gameinfo.txt");
        QVERIFY(QDir().mkpath(library + "/steamapps/compatdata/4000/pfx"));

        writeFile(root + "/steamapps/appmanifest_440.acf",
                  "\"AppState\"\n{\n\"appid\" \"440\"\n\"name\" \"Team Fortress 2\"\n\"installdir\" \"Team Fortress 2\"\n}\n");
        writeFile(root + "/steamapps/common/Team Fortress 2/tf.sh");
        writeFile(root + "/steamapps/common/Team Fortress 2/tf/GameInfo.txt");

        writeFile(root + "/steamapps/appmanifest_10.acf",
                  "\"AppState\"\n{\n\"appid\" \"10\"\n\"name\" \"Uninstalled\"\n\"installdir\" \"Gone\"\n}\n");

        const steam::Installation installation = steam::scan(root);
        QCOMPARE(installation.libraries, QStringList({root, library}));
        QCOMPARE(installation.apps.size(), 2);

        const steam::App *gmod = findApp(installation, 4000);
        QVERIFY(gmod);
        QCOMPARE(gmod->platform, steam::Platform::Windows);
        QCOMPARE(gmod->gameDirs, QStringList({library + "/steamapps/common/GarrysMod/garrysmod"}));
        QCOMPARE(gmod->toolsPath, library + "/steamapps/common/GarrysMod/bin");
        QCOMPARE(gmod->compatDataPath, library + "/steamapps/compatdata/4000");
        QCOMPARE(gmod->compatToolName, "GE-Test");
        QCOMPARE(gmod->protonPath, root + "/compatibilitytools.d/GE-Test");

        const steam::App *tf2 = findApp(installation, 440);
        QVERIFY(tf2);
        QCOMPARE(tf2->platform, steam::Platform::Linux);
        QCOMPARE(tf2->gameDirs.size(), 1);
        QVERIFY(tf2->toolsPath.isEmpty());
        QVERIFY(tf2->protonPath.isEmpty());
    }

    void toolsOverride()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        const QString dir = QDir(temp.path()).canonicalPath();
        writeFile(dir + "/win/bin/studiomdl.exe");
        writeFile(dir + "/native/bin/tool");

        const QList<steam::CompatTool> tools = {{"GE-Proton", "GE", dir + "/ge"},
                                                {"proton_9", "Proton 9.0", dir + "/p9"},
                                                {"proton_experimental", "Experimental", dir + "/exp"}};
        steam::App app;
        steam::overrideTools(app, dir + "/missing", tools, dir + "/prefix");
        QVERIFY(!app.toolsOverridden);

        steam::overrideTools(app, dir + "/win/bin/", tools, dir + "/prefix");
        QVERIFY(app.toolsOverridden);
        QCOMPARE(app.toolsPath, dir + "/win/bin");
        QCOMPARE(app.compatToolName, "proton_experimental");
        QCOMPARE(app.compatDataPath, dir + "/prefix");
        QVERIFY(QDir(dir + "/prefix").exists());

        steam::App native;
        steam::overrideTools(native, dir + "/native/bin", tools, dir + "/prefix2");
        QVERIFY(native.protonPath.isEmpty());

        steam::App own;
        own.protonPath = dir + "/mine";
        steam::overrideTools(own, dir + "/win/bin", tools, dir + "/prefix3");
        QCOMPARE(own.protonPath, dir + "/mine");

        steam::App custom;
        steam::overrideTools(custom, dir + "/win/bin", {tools[0]}, dir + "/prefix4");
        QCOMPARE(custom.compatToolName, "GE-Proton");
    }

    void protonFallsBackToPrefixConfigInfo()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        const QString root = QDir(temp.path()).canonicalPath() + "/Steam";
        const QString proton = QDir(temp.path()).canonicalPath() + "/tools/SomeProton";

        writeFile(proton + "/proton");
        writeFile(root + "/steamapps/appmanifest_4000.acf",
                  "\"AppState\"\n{\n\"appid\" \"4000\"\n\"name\" \"Garry's Mod\"\n\"installdir\" \"GarrysMod\"\n}\n");
        writeFile(root + "/steamapps/common/GarrysMod/gmod.exe");
        writeFile(root + "/steamapps/common/GarrysMod/garrysmod/gameinfo.txt");
        QVERIFY(QDir().mkpath(root + "/steamapps/compatdata/4000/pfx"));
        writeFile(root + "/steamapps/compatdata/4000/config_info",
                  QStringLiteral("SomeProton\n%1/files/share/fonts/\n%1/files/lib/\n").arg(proton).toUtf8());

        const steam::Installation installation = steam::scan(root);
        const steam::App *gmod = findApp(installation, 4000);
        QVERIFY(gmod);
        QCOMPARE(gmod->protonPath, proton);
        QCOMPARE(gmod->compatToolName, "SomeProton");
    }
};

QTEST_APPLESS_MAIN(TestSteam)
#include "tst_steam.moc"
