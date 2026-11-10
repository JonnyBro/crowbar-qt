#include "core/Package.h"
#include "core/Packer.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QtEndian>

namespace {

template <typename T>
void put(QByteArray &data, T value)
{
    char buffer[sizeof(T)];
    qToLittleEndian<T>(value, buffer);
    data.append(buffer, sizeof(T));
}

void putString(QByteArray &data, const char *text)
{
    data.append(text);
    data.append('\0');
}

void putVpkEntry(QByteArray &tree, const char *name, quint16 archive, quint32 offset, quint32 length,
                 const QByteArray &preload = {})
{
    putString(tree, name);
    put<quint32>(tree, 0);
    put<quint16>(tree, quint16(preload.size()));
    put<quint16>(tree, archive);
    put<quint32>(tree, offset);
    put<quint32>(tree, length);
    put<quint16>(tree, 0xffff);
    tree.append(preload);
}

void writeFile(const QString &path, const QByteArray &content)
{
    QDir().mkpath(QFileInfo(path).absolutePath());
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(content);
}

QByteArray readFile(const QString &path)
{
    QFile file(path);
    return file.open(QIODevice::ReadOnly) ? file.readAll() : QByteArray("<missing>");
}

packer::Options multiChunk()
{
    packer::Options options;
    options.multiChunk = true;
    return options;
}

QByteArray vpkTree()
{
    QByteArray tree;
    putString(tree, "mdl");
    putString(tree, "models/props");
    putVpkEntry(tree, "crate", 0x7fff, 0, 5);
    putVpkEntry(tree, "barrel", 1, 3, 4, "PRE");
    putString(tree, "");
    putString(tree, "");
    putString(tree, "txt");
    putString(tree, " ");
    putVpkEntry(tree, "readme", 0x7fff, 5, 2);
    putString(tree, "");
    putString(tree, "");
    putString(tree, "");
    return tree;
}

} // namespace

class TestPackage : public QObject {
    Q_OBJECT

private slots:
    void vpk_data()
    {
        QTest::addColumn<int>("version");
        QTest::newRow("v1") << 1;
        QTest::newRow("v2") << 2;
    }

    void vpk()
    {
        QFETCH(int, version);
        QTemporaryDir temp;
        QVERIFY(temp.isValid());

        const QByteArray tree = vpkTree();
        QByteArray dir;
        put<quint32>(dir, 0x55aa1234);
        put<quint32>(dir, quint32(version));
        put<quint32>(dir, quint32(tree.size()));
        if (version == 2) {
            for (int i = 0; i < 4; ++i)
                put<quint32>(dir, 0);
        }
        dir += tree;
        dir += "CRATEok";
        writeFile(temp.path() + "/pak_dir.vpk", dir);
        writeFile(temp.path() + "/pak_001.vpk", "xxxBARRzzz");

        QString error;
        // Opening a numbered part must redirect to the directory file.
        const auto package = package::Package::open(temp.path() + "/pak_001.vpk", &error);
        QVERIFY2(package, qPrintable(error));
        QCOMPARE(package->format(), QStringLiteral("VPK v%1").arg(version));
        QCOMPARE(package->entries().size(), 3);
        QCOMPARE(package->entries()[0].path, "models/props/crate.mdl");
        QCOMPARE(package->entries()[1].path, "models/props/barrel.mdl");
        QCOMPARE(package->entries()[1].size, quint64(7));
        QCOMPARE(package->entries()[2].path, "readme.txt");

        const QString out = temp.path() + "/out";
        for (const package::Entry &entry : package->entries())
            QVERIFY2(package->extract(entry, out, &error), qPrintable(error));
        QCOMPARE(readFile(out + "/models/props/crate.mdl"), "CRATE");
        QCOMPARE(readFile(out + "/models/props/barrel.mdl"), "PREBARR");
        QCOMPARE(readFile(out + "/readme.txt"), "ok");
    }

    void gma()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());

        QByteArray gma("GMAD");
        put<quint8>(gma, 3);
        put<quint64>(gma, 0);
        put<quint64>(gma, 0);
        putString(gma, "");
        putString(gma, "My Addon");
        putString(gma, "{}");
        putString(gma, "Author");
        put<qint32>(gma, 1);
        put<quint32>(gma, 1);
        putString(gma, "lua/autorun/a.lua");
        put<qint64>(gma, 5);
        put<quint32>(gma, 0);
        put<quint32>(gma, 2);
        putString(gma, "../escape.txt");
        put<qint64>(gma, 3);
        put<quint32>(gma, 0);
        put<quint32>(gma, 0);
        gma += "helloBAD";
        writeFile(temp.path() + "/addon.gma", gma);

        QString error;
        const auto package = package::Package::open(temp.path() + "/addon.gma", &error);
        QVERIFY2(package, qPrintable(error));
        QCOMPARE(package->format(), "GMA v3");
        QCOMPARE(package->title(), "My Addon");
        QCOMPARE(package->entries().size(), 2);

        const QString out = temp.path() + "/nested/out";
        package::ExtractJob job(*package, {0, 1}, out);
        QSignalSpy done(&job, &package::ExtractJob::done);
        QSignalSpy messages(&job, &package::ExtractJob::message);
        job.start();
        QVERIFY(done.wait(5000));
        job.wait();
        QCOMPARE(done[0][0].toInt(), 1);
        QCOMPARE(done[0][1].toInt(), 1);
        QCOMPARE(messages.size(), 1);
        QCOMPARE(readFile(out + "/lua/autorun/a.lua"), "hello");
        QVERIFY(!QFile::exists(temp.path() + "/nested/escape.txt"));
    }

    void rejectsOtherFiles()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        writeFile(temp.path() + "/a.vpk", "not a package");
        QString error;
        QVERIFY(!package::Package::open(temp.path() + "/a.vpk", &error));
        QVERIFY(!error.isEmpty());
        QVERIFY(!package::Package::open(temp.path() + "/missing.vpk", &error));

        QByteArray truncated;
        put<quint32>(truncated, 0x55aa1234);
        put<quint32>(truncated, 1);
        put<quint32>(truncated, 500);
        writeFile(temp.path() + "/b.vpk", truncated);
        QVERIFY(!package::Package::open(temp.path() + "/b.vpk", &error));
    }

    void packCommands()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        const QString dir = temp.path();
        writeFile(dir + "/bin/vpk.exe", "MZ");
        writeFile(dir + "/bin/gmad.exe", "MZ");
        writeFile(dir + "/addon/lua/a.lua", "x");

        steam::App app;
        app.id = 4000;
        app.toolsPath = dir + "/bin";
        app.protonPath = dir + "/proton";
        app.compatDataPath = dir + "/compatdata";

        launcher::Command command = packer::packCommand(app, packer::Format::Vpk, dir + "/addon", {});
        QVERIFY(command.isValid());
        QCOMPARE(command.arguments, QStringList({"runinprefix", dir + "/bin/vpk.exe", launcher::toWindowsPath(dir + "/addon")}));
        command = packer::packCommand(app, packer::Format::Vpk, dir + "/addon/", {}, multiChunk());
        QCOMPARE(command.arguments.mid(2), QStringList({"-M", launcher::toWindowsPath(dir + "/addon")}));

        command = packer::packCommand(app, packer::Format::Gma, dir + "/addon", dir + "/out/a.gma");
        QVERIFY(!command.isValid());
        QVERIFY(command.error.contains("addon.json"));

        writeFile(dir + "/addon/addon.json", "{}");
        command = packer::packCommand(app, packer::Format::Gma, dir + "/addon", dir + "/out/a.gma");
        QVERIFY(command.isValid());
        QCOMPARE(command.arguments.mid(2),
                 QStringList({"create", "-folder", launcher::toWindowsPath(dir + "/addon"), "-out",
                              launcher::toWindowsPath(dir + "/out/a.gma")}));

        const QDateTime started = QDateTime::currentDateTime();
        packer::Options flags;
        flags.warnInvalid = true;
        flags.extra = {"-quiet"};
        command = packer::packCommand(app, packer::Format::Gma, dir + "/addon", dir + "/out/a.gma", flags);
        QCOMPARE(command.arguments.mid(7), QStringList({"-warninvalid", "-quiet"}));
        flags.multiChunk = true;
        flags.extra = {"-c", "100"};
        command = packer::packCommand(app, packer::Format::Vpk, dir + "/addon", {}, flags);
        QCOMPARE(command.arguments.mid(2), QStringList({"-M", "-c", "100", launcher::toWindowsPath(dir + "/addon")}));

        QStringList log;
        QVERIFY(packer::finishPack(packer::Format::Vpk, dir + "/addon", dir + "/elsewhere/final.vpk", {}, started, log).isEmpty());
        writeFile(dir + "/addon.vpk", "packed");
        QCOMPARE(packer::finishPack(packer::Format::Vpk, dir + "/addon", dir + "/elsewhere/final.vpk", {}, started, log),
                 QStringList({dir + "/elsewhere/final.vpk"}));
        QCOMPARE(readFile(dir + "/elsewhere/final.vpk"), "packed");
        QVERIFY(!QFile::exists(dir + "/addon.vpk"));

        writeFile(dir + "/addon_dir.vpk", "d");
        writeFile(dir + "/addon_000.vpk", "0");
        writeFile(dir + "/addon_001.vpk", "1");
        writeFile(dir + "/addon_extra_dir.vpk", "other");
        QCOMPARE(packer::finishPack(packer::Format::Vpk, dir + "/addon", dir + "/multi/pak01.vpk", multiChunk(), started, log),
                 QStringList({dir + "/multi/pak01_000.vpk", dir + "/multi/pak01_001.vpk", dir + "/multi/pak01_dir.vpk"}));
        QVERIFY(QFile::exists(dir + "/addon_extra_dir.vpk"));

        // Chunks older than this run belong to a previous build.
        writeFile(dir + "/addon_dir.vpk", "stale");
        QVERIFY(packer::finishPack(packer::Format::Vpk, dir + "/addon", dir + "/multi2/a.vpk", multiChunk(),
                                   started.addSecs(60), log).isEmpty());
    }

    void addonJson()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        QVERIFY(!packer::hasAddonJson(temp.path()));
        QVERIFY(packer::writeAddonJson(temp.path(), "My \"Addon\"", "model", {"fun", "build"}));
        QVERIFY(packer::hasAddonJson(temp.path()));

        const QJsonObject json = QJsonDocument::fromJson(readFile(temp.path() + "/addon.json")).object();
        QCOMPARE(json["title"].toString(), "My \"Addon\"");
        QCOMPARE(json["type"].toString(), "model");
        QCOMPARE(json["tags"].toArray().size(), 2);
        QVERIFY(json["ignore"].isArray());

        QVERIFY(!packer::writeAddonJson(temp.path(), "Other", "map", {}));
        QCOMPARE(QJsonDocument::fromJson(readFile(temp.path() + "/addon.json")).object()["type"].toString(), "model");
        QVERIFY(packer::addonTypes().contains("servercontent"));
        QCOMPARE(packer::addonTags().size(), 9);
    }
};

QTEST_GUILESS_MAIN(TestPackage)
#include "tst_package.moc"
