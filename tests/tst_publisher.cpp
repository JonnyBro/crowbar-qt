#include "core/Publisher.h"

#include <QDir>
#include <QFile>
#include <QImage>
#include <QImageReader>
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

class TestPublisher : public QObject {
    Q_OBJECT

private slots:
    void parseList()
    {
        publisher::ListParser parser;
        for (const char *line : {"Garry's Mod Workshop Publisher 1.2", "Getting published files..",
                                 "\t3835381011\t145.0 KB\t\"Beatrun Weapon HUD\"",
                                 "\t3030826644\t12.8 MB\t\"MW 2019 SMG | Compressed 512x512", "\"",
                                 "\t3030826521\t49.7 MB\t\"Two", "lines\"", "\t42\t1 B\t\"\""})
            parser.addLine(QString::fromUtf8(line));
        const QList<publisher::WorkshopItem> items = parser.items();
        QCOMPARE(items.size(), 4);
        QCOMPARE(items[0].id, quint64(3835381011));
        QCOMPARE(items[0].size, "145.0 KB");
        QCOMPARE(items[0].title, "Beatrun Weapon HUD");
        QCOMPARE(items[1].title, "MW 2019 SMG | Compressed 512x512");
        QCOMPARE(items[2].title, "Two lines");
        QCOMPARE(items[3].title, QString());
    }

    void createdId()
    {
        QCOMPARE(publisher::parseCreatedId("Success! Your new item ID is 3835381011!"), quint64(3835381011));
        QCOMPARE(publisher::parseCreatedId("Success!"), quint64(0));
    }

    void commands()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        const QString dir = temp.path();
        writeFile(dir + "/bin/gmpublish.exe", "MZ");
        writeFile(dir + "/a.gma", "GMAD");
        writeFile(dir + "/icon.jpg", "x");

        steam::App app;
        app.id = 4000;
        app.toolsPath = dir + "/bin";
        app.protonPath = dir + "/proton";
        app.compatDataPath = dir + "/compatdata";

        launcher::Command command = publisher::listCommand(app);
        QCOMPARE(command.arguments, QStringList({"runinprefix", dir + "/bin/gmpublish.exe", "list"}));

        publisher::Request request;
        request.gmaFile = dir + "/a.gma";
        QVERIFY(!publisher::publishCommand(app, request).isValid());
        request.iconFile = dir + "/icon.jpg";
        request.changes = "ignored for create";
        command = publisher::publishCommand(app, request);
        QVERIFY(command.isValid());
        QCOMPARE(command.arguments.mid(2), QStringList({"create", "-addon", launcher::toWindowsPath(dir + "/a.gma"),
                                                        "-icon", launcher::toWindowsPath(dir + "/icon.jpg")}));

        request.id = 1337;
        request.iconFile.clear();
        request.changes = " fixed something ";
        command = publisher::publishCommand(app, request);
        QCOMPARE(command.arguments.mid(2), QStringList({"update", "-id", "1337", "-addon",
                                                        launcher::toWindowsPath(dir + "/a.gma"), "-changes", "fixed something"}));

        request.gmaFile.clear();
        QVERIFY(!publisher::publishCommand(app, request).isValid());
        request.gmaFile = dir + "/missing.gma";
        QVERIFY(!publisher::publishCommand(app, request).isValid());
    }

    void icon()
    {
        QTemporaryDir temp;
        QVERIFY(temp.isValid());
        QImage png(800, 400, QImage::Format_ARGB32);
        png.fill(Qt::transparent);
        QVERIFY(png.save(temp.filePath("wide.png")));

        QString error;
        const QString converted = publisher::prepareIcon(temp.filePath("wide.png"), temp.filePath("out.jpg"), &error);
        QCOMPARE(converted, temp.filePath("out.jpg"));
        QImageReader reader(converted);
        QCOMPARE(reader.format(), "jpeg");
        QCOMPARE(reader.size(), QSize(512, 512));
        const QImage result = reader.read();
        QVERIFY(qGray(result.pixel(256, 256)) > 240); // transparency became white

        // An already valid icon is used as is.
        QCOMPARE(publisher::prepareIcon(converted, temp.filePath("again.jpg"), &error), converted);
        QVERIFY(!QFile::exists(temp.filePath("again.jpg")));

        QVERIFY(publisher::prepareIcon(temp.filePath("missing.png"), temp.filePath("x.jpg"), &error).isEmpty());
        QVERIFY(!error.isEmpty());
    }
};

QTEST_GUILESS_MAIN(TestPublisher)
#include "tst_publisher.moc"
