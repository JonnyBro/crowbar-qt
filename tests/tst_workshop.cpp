#include "core/Workshop.h"

#include <QDir>
#include <QFile>
#include <QProcess>
#include <QTemporaryDir>
#include <QTest>

namespace
{

	void writeFile(const QString &path, const QByteArray &content = {})
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

} // namespace

class TestWorkshop : public QObject
{
	Q_OBJECT

private slots:
	void itemIds()
	{
		QCOMPARE(workshop::parseItemId(" 104603291 "), quint64(104603291));
		QCOMPARE(workshop::parseItemId("https://steamcommunity.com/sharedfiles/filedetails/?id=104603291&searchtext="),
				 quint64(104603291));
		QCOMPARE(workshop::parseItemId("https://steamcommunity.com/workshop/filedetails/?l=en&id=42"), quint64(42));
		QCOMPARE(workshop::parseItemId("not an id"), quint64(0));
	}

	void itemIdLists()
	{
		QCOMPARE(workshop::parseItemIds("104603291 https://steamcommunity.com/sharedfiles/filedetails/?id=42, 7;\n104603291"),
				 QList<quint64>({104603291, 42, 7}));
		QVERIFY(workshop::parseItemIds("  nothing here ").isEmpty());
	}

	void collections()
	{
		bool isCollection = false;
		const QList<quint64> children = workshop::parseCollection(
			"{\"response\":{\"collectiondetails\":[{\"publishedfileid\":\"9\",\"result\":1,\"children\":["
			"{\"publishedfileid\":\"1\",\"filetype\":0},{\"publishedfileid\":\"2\",\"filetype\":2},{\"publishedfileid\":\"3\",\"filetype\":0}]}]}}",
			&isCollection);
		QVERIFY(isCollection);
		QCOMPARE(children, QList<quint64>({1, 3}));
		QVERIFY(workshop::parseCollection("{\"response\":{\"collectiondetails\":[{\"publishedfileid\":\"5\",\"result\":9}]}}", &isCollection).isEmpty());
		QVERIFY(!isCollection);
	}

	void details()
	{
		const auto ok = workshop::parseDetails(R"({"response":{"result":1,"resultcount":1,"publishedfiledetails":[
            {"publishedfileid":"104603291","result":1,"consumer_app_id":4000,"title":"Extended Spawnmenu",
             "filename":"","file_url":"","file_size":"42347","time_updated":1785371010}]}})");
		QVERIFY(ok.isValid());
		QCOMPARE(ok.appId, quint32(4000));
		QCOMPARE(ok.title, "Extended Spawnmenu");
		QCOMPARE(ok.fileSize, 42347);
		QVERIFY(ok.fileUrl.isEmpty());

		// Plain string on purpose: moc reads "//" inside a raw string as a comment.
		const auto list = workshop::parseDetailsList("{\"response\":{\"publishedfiledetails\":["
													 "{\"publishedfileid\":\"1\",\"result\":1,\"title\":\"A\",\"preview_url\":\"https://x/a.jpg\"},"
													 "{\"publishedfileid\":\"2\",\"result\":9}]}}");
		QCOMPARE(list.size(), 2);
		QCOMPARE(list[0].previewUrl, "https://x/a.jpg");
		QVERIFY(list[0].isValid());
		QVERIFY(!list[1].isValid());

		const auto missing = workshop::parseDetails(R"({"response":{"publishedfiledetails":[{"publishedfileid":"5","result":9}]}})");
		QVERIFY(!missing.isValid());
		QVERIFY(!workshop::parseDetails("garbage").isValid());
	}

	void names()
	{
		workshop::ItemDetails details;
		details.id = 7;
		details.title = "  MW 2019: Rigs | <Compressed>\n512x512 ";
		QCOMPARE(workshop::fileBaseName(details, true), "MW 2019_ Rigs _ _Compressed_ 512x512 (7)");
		QCOMPARE(workshop::fileBaseName(details, false), "7");
		QCOMPARE(workshop::parseSteamCmdPath("Success. Downloaded item 42 to \"/x/content/4000/42\" (10 bytes)"),
				 "/x/content/4000/42");
	}

	void steamCmdRunsInItsOwnHome()
	{
		QTemporaryDir temp;
		QVERIFY(temp.isValid());
		qputenv("STEAM_COMPAT_CLIENT_INSTALL_PATH", "/home/user/.local/share/Steam");
		qputenv("SteamAppId", "4000");
		const QString home = temp.path() + "/home";
		const launcher::Command command = workshop::steamCmdCommand("/usr/bin/steamcmd", home, temp.path() + "/out", 4000, 42);
		QVERIFY(command.isValid());
		QCOMPARE(command.environment.value("HOME"), home);
		QCOMPARE(command.environment.value("XDG_DATA_HOME"), home + "/.local/share");
		QVERIFY(!command.environment.contains("STEAM_COMPAT_CLIENT_INSTALL_PATH"));
		QVERIFY(!command.environment.contains("SteamAppId"));
		QVERIFY(QDir(home).exists());
		QCOMPARE(command.arguments.mid(0, 2), QStringList({"+force_install_dir", temp.path() + "/out"}));
		QVERIFY(!workshop::steamCmdCommand({}, home, temp.path(), 4000, 42).isValid());
	}

	void cacheAndSave()
	{
		QTemporaryDir temp;
		QVERIFY(temp.isValid());
		const QString lib = temp.path() + "/lib";
		writeFile(lib + "/steamapps/workshop/content/4000/42/addon.gma", "GMAD plain");
		QCOMPARE(workshop::findCachedFiles({temp.path() + "/empty", lib}, 4000, 42),
				 QStringList({lib + "/steamapps/workshop/content/4000/42/addon.gma"}));
		QVERIFY(workshop::findCachedFiles({lib}, 4000, 43).isEmpty());

		QString error;
		const QString out = temp.path() + "/out";
		QCOMPARE(workshop::saveFile(lib + "/steamapps/workshop/content/4000/42/addon.gma", out, "Name (42)", true, &error),
				 out + "/Name (42).gma");
		QCOMPARE(readFile(out + "/Name (42).gma"), "GMAD plain");

		// An LZMA-compressed GMA becomes a plain .gma; other compressed data stays .bin.
		writeFile(temp.path() + "/plain.gma", "GMAD compressed addon");
		writeFile(temp.path() + "/dupe.txt", "DUP3 not an addon");
		for (const char *name : {"plain.gma", "dupe.txt"})
		{
			QCOMPARE(QProcess::execute("xz", {"--format=lzma", "-k", "-f", temp.path() + '/' + name}), 0);
		}
		QCOMPARE(workshop::saveFile(temp.path() + "/plain.gma.lzma", out, "a", true, &error), out + "/a.gma");
		QCOMPARE(readFile(out + "/a.gma"), "GMAD compressed addon");
		QCOMPARE(workshop::saveFile(temp.path() + "/dupe.txt.lzma", out, "b", true, &error), out + "/b.bin");
		QCOMPARE(readFile(out + "/b.bin"), "DUP3 not an addon");
		QCOMPARE(workshop::saveFile(temp.path() + "/dupe.txt.lzma", out, "c", false, &error), out + "/c.lzma");
	}
};

QTEST_GUILESS_MAIN(TestWorkshop)
#include "tst_workshop.moc"
