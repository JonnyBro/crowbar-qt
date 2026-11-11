#include "core/Hlmv.h"
#include "core/Package.h"
#include "core/Packer.h"
#include "core/Publisher.h"
#include "core/Runner.h"
#include "core/Workshop.h"
#include "core/Steam.h"
#include "core/Studiomdl.h"
#include "core/ToolProcess.h"
#include "ui/GameList.h"
#include "ui/MainWindow.h"
#include "ui/SettingsPage.h"
#include "ui/TempDir.h"

#include <QApplication>
#include <QSettings>
#include <QSocketNotifier>
#include <QCoreApplication>
#include <QFileInfo>
#include <QProcess>
#include <QIcon>
#include <QLibraryInfo>
#include <QLocale>
#include <QTemporaryDir>
#include <QTranslator>
#include <QTextStream>

#include <cstring>

#include <signal.h>
#include <sys/socket.h>
#include <unistd.h>

namespace
{
	int listGames()
	{
		// Same list as the window: mods, added folders and chosen tools folders included.
		GameList games;
		games.refresh();
		QTextStream out(stdout);
		for (const QString &library : games.libraries())
			out << "library: " << library << '\n';
		for (const steam::CompatTool &tool : games.compatTools())
			out << "compat tool: " << tool.name << " -> " << tool.path << '\n';
		for (const steam::App &app : games.apps())
		{
			out << '\n'
				<< app.name << " (" << app.id << ")" << (app.isMod ? QStringLiteral(" mod of ") + (app.baseName.isEmpty() ? QStringLiteral("?") : app.baseName) : QString()) << '\n';
			out << "  build:   " << (app.platform == steam::Platform::Windows ? "windows" : "linux") << '\n';
			out << "  install: " << app.installPath << '\n';
			out << "  tools:   " << (app.toolsPath.isEmpty() ? QStringLiteral("-") : app.toolsPath) << (app.toolsOverridden ? " (custom)" : "") << '\n';
			if (!app.protonPath.isEmpty() || app.platform == steam::Platform::Windows)
			{
				out << "  proton:  " << (app.protonPath.isEmpty() ? QStringLiteral("not found") : app.protonPath)
					<< " [" << app.compatToolName << "]\n";
				out << "  prefix:  " << (app.compatDataPath.isEmpty() ? QStringLiteral("-") : app.compatDataPath) << '\n';
			}
			for (const QString &gameDir : app.gameDirs)
				out << "  game:    " << gameDir << '\n';
		}
		return 0;
	}

	int run(QCoreApplication &app, const launcher::Command &command)
	{
		QTextStream out(stdout);
		QTextStream err(stderr);
		if (!command.isValid())
		{
			err << command.error << Qt::endl;
			return 1;
		}
		ToolProcess process;
		QObject::connect(&process, &ToolProcess::outputLine, [&](const QString &line)
						 { out << line << Qt::endl; });
		QObject::connect(&process, &ToolProcess::failedToStart, [&](const QString &error)
						 {
		err << error << Qt::endl;
		app.exit(1); });
		QObject::connect(&process, &ToolProcess::finished, [&](int exitCode, bool success)
						 { app.exit(success ? 0 : (exitCode ? exitCode : 1)); });
		process.start(command);
		return app.exec();
	}

	std::optional<steam::App> findApp(quint32 appId)
	{
		// Same list as the window, so tools folders chosen there apply here too.
		GameList games;
		games.refresh();
		for (const steam::App &game : games.apps())
		{
			if (game.id == appId)
				return game;
		}
		QTextStream(stderr) << "App " << appId << " is not installed." << Qt::endl;
		return std::nullopt;
	}

	int listPackage(const QString &file)
	{
		QString error;
		const auto package = package::Package::open(file, &error);
		if (!package)
		{
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
		if (!package)
		{
			err << error << Qt::endl;
			return 1;
		}
		int failed = 0;
		for (const package::Entry &entry : package->entries())
		{
			if (!package->extract(entry, outputDir, &error))
			{
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
		for (const QString &flag : flags)
		{
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

	int workshopList(QCoreApplication &app, quint32 appId)
	{
		const auto game = findApp(appId);
		if (!game)
			return 1;
		publisher::ListParser parser;
		ToolProcess process;
		QObject::connect(&process, &ToolProcess::outputLine, [&](const QString &line)
						 { parser.addLine(line); });
		QObject::connect(&process, &ToolProcess::failedToStart, [&](const QString &error)
						 {
		QTextStream(stderr) << error << Qt::endl;
		app.exit(1); });
		QObject::connect(&process, &ToolProcess::finished, [&](int exitCode, bool)
						 { app.exit(exitCode); });
		const launcher::Command command = publisher::listCommand(*game);
		if (!command.isValid())
		{
			QTextStream(stderr) << command.error << Qt::endl;
			return 1;
		}
		process.start(command);
		const int result = app.exec();
		QTextStream out(stdout);
		for (const publisher::WorkshopItem &item : parser.items())
			out << item.id << '\t' << item.size << '\t' << item.title << '\n';
		return result;
	}

	int publish(QCoreApplication &app, quint32 appId, const QStringList &args)
	{
		const auto game = findApp(appId);
		if (!game)
			return 1;
		publisher::Request request;
		for (int i = 0; i + 1 < args.size(); i += 2)
		{
			if (args[i] == "--id")
				request.id = args[i + 1].toULongLong();
			else if (args[i] == "--addon")
				request.gmaFile = args[i + 1];
			else if (args[i] == "--icon")
				request.iconFile = args[i + 1];
			else if (args[i] == "--changes")
				request.changes = args[i + 1];
		}
		if (!request.iconFile.isEmpty())
		{
			static QTemporaryDir temp;
			QString error;
			request.iconFile = publisher::prepareIcon(request.iconFile, temp.filePath("icon.jpg"), &error);
			if (request.iconFile.isEmpty())
			{
				QTextStream(stderr) << error << Qt::endl;
				return 1;
			}
		}
		return run(app, publisher::publishCommand(*game, request));
	}

	int download(QCoreApplication &app, const QString &item, const QString &outputDir)
	{
		QTextStream out(stdout);
		QTextStream err(stderr);
		const quint64 id = workshop::parseItemId(item);
		if (!id)
		{
			err << "Not a workshop link or ID: " << item << Qt::endl;
			return 2;
		}
		GameList games;
		games.refresh();
		workshop::Client client;
		QTemporaryDir temp;
		workshop::ItemDetails details;
		QStringList files;

		QObject::connect(&client, &workshop::Client::detailsReady, [&](const workshop::ItemDetails &result)
						 {
		details = result;
		if (!details.isValid()) {
			err << details.error << Qt::endl;
			app.exit(1);
			return;
		}
		out << details.title << " (app " << details.appId << ")" << Qt::endl;
		files = workshop::findCachedFiles(games.libraries(), details.appId, id);
		if (!files.isEmpty()) {
			out << "From Steam's workshop cache." << Qt::endl;
			app.exit(0);
		} else if (!details.fileUrl.isEmpty()) {
			out << "Downloading " << details.fileUrl << Qt::endl;
			client.download(details.fileUrl, temp.filePath("item.download"));
		} else {
			err << "No direct link and not in Steam's cache; subscribe in Steam or use the window with steamcmd."
				<< Qt::endl;
			app.exit(1);
		} });
		QObject::connect(&client, &workshop::Client::downloaded, [&](bool ok, const QString &error)
						 {
		if (!ok)
			err << error << Qt::endl;
		else
			files = {temp.filePath("item.download")};
		app.exit(ok ? 0 : 1); });
		client.fetchDetails(id);
		if (app.exec() != 0)
			return 1;

		const QString baseName = workshop::fileBaseName(details, true);
		int failed = 0;
		for (int i = 0; i < files.size(); ++i)
		{
			QString error;
			const QString name = files.size() == 1 ? baseName : QStringLiteral("%1_%2").arg(baseName).arg(i + 1);
			const QString saved = workshop::saveFile(files[i], outputDir, name, true, &error);
			if (saved.isEmpty())
			{
				err << error << Qt::endl;
				++failed;
			}
			else
			{
				out << "Saved " << saved << Qt::endl;
			}
		}
		return failed ? 1 : 0;
	}

	int view(QCoreApplication &app, quint32 appId, const QString &gameDir, const QString &mdlFile)
	{
		const auto game = findApp(appId);
		return game ? run(app, hlmv::viewCommand(*game, gameDir, mdlFile)) : 1;
	}

	int compile(QCoreApplication &app, quint32 appId, const QString &gameDir, const QString &qcFile,
				const QString &outputDir)
	{
		QTextStream out(stdout);
		QTextStream err(stderr);
		if (const auto found = findApp(appId))
		{
			const steam::App &game = *found;
			{
				ToolProcess process;
				QObject::connect(&process, &ToolProcess::outputLine, [&](const QString &line)
								 { out << line << Qt::endl; });
				QObject::connect(&process, &ToolProcess::failedToStart, [&](const QString &error)
								 {
				err << error << Qt::endl;
				app.exit(1); });
				QObject::connect(&process, &ToolProcess::finished, [&](int exitCode, bool success)
								 {
				if (success && !outputDir.isEmpty()) {
					QStringList log;
					success = studiomdl::moveCompiledModel(gameDir, studiomdl::readModelName(qcFile), outputDir, log);
					for (const QString &line : log)
						out << line << Qt::endl;
				}
				app.exit(success ? 0 : (exitCode ? exitCode : 1)); });
				const launcher::Command command = studiomdl::compileCommand(game, gameDir, qcFile, {});
				if (!command.isValid())
				{
					err << command.error << Qt::endl;
					return 1;
				}
				process.start(command);
				return app.exec();
			}
		}
		return 1;
	}

	// Ctrl+C, a session logout or kill should close the window the normal way, so
	// destructors run: tools started from it are stopped and the temp folder goes.
	int signalPipe[2] = {-1, -1};

	void quitOnSignals()
	{
		if (::socketpair(AF_UNIX, SOCK_STREAM, 0, signalPipe) != 0)
			return;
		auto *notifier = new QSocketNotifier(signalPipe[1], QSocketNotifier::Read, qApp);
		QObject::connect(notifier, &QSocketNotifier::activated, qApp, []
						 {
			char byte = 0;
			if (::read(signalPipe[1], &byte, 1) >= 0)
				QCoreApplication::quit(); });
		struct sigaction action = {};
		action.sa_handler = [](int)
		{
			const char byte = 1;
			// Only async-signal-safe calls here; the notifier does the rest.
			[[maybe_unused]] const auto written = ::write(signalPipe[0], &byte, 1);
		};
		sigemptyset(&action.sa_mask);
		action.sa_flags = SA_RESTART;
		for (int signal : {SIGINT, SIGTERM, SIGHUP})
			::sigaction(signal, &action, nullptr);
	}

} // namespace

int main(int argc, char *argv[])
{
	QCoreApplication::setApplicationName("crowbar-qt");
	QCoreApplication::setOrganizationName("crowbar-qt");
	QCoreApplication::setApplicationVersion(CROWBAR_VERSION);

	// Tool environment from the settings page also applies to the command-line modes.
	SettingsPage::applySaved();

	// Checked before QApplication exists so the listing works without a display.
	for (int i = 1; i < argc; ++i)
	{
		if (std::strcmp(argv[i], "--list-games") == 0)
		{
			QCoreApplication app(argc, argv);
			return listGames();
		}
		if (std::strcmp(argv[i], "--list-package") == 0 && i + 1 < argc)
		{
			QCoreApplication app(argc, argv);
			return listPackage(QString::fromLocal8Bit(argv[i + 1]));
		}
		if (std::strcmp(argv[i], "--unpack") == 0 && i + 2 < argc)
		{
			QCoreApplication app(argc, argv);
			return unpack(QString::fromLocal8Bit(argv[i + 1]), QString::fromLocal8Bit(argv[i + 2]));
		}
		if (std::strcmp(argv[i], "--pack") == 0)
		{
			if (i + 4 >= argc)
			{
				QTextStream(stderr) << "Usage: crowbar-qt --pack <appid> <vpk|gma> <folder> <output file> [--multi] [packer options]\n";
				return 2;
			}
			QCoreApplication app(argc, argv);
			return pack(app, QString::fromLocal8Bit(argv[i + 1]).toUInt(), QString::fromLocal8Bit(argv[i + 2]),
						QString::fromLocal8Bit(argv[i + 3]), QString::fromLocal8Bit(argv[i + 4]),
						QCoreApplication::arguments().mid(i + 5));
		}
		if (std::strcmp(argv[i], "--workshop-list") == 0 && i + 1 < argc)
		{
			QCoreApplication app(argc, argv);
			return workshopList(app, QString::fromLocal8Bit(argv[i + 1]).toUInt());
		}
		if (std::strcmp(argv[i], "--publish") == 0)
		{
			if (i + 1 >= argc)
			{
				QTextStream(stderr) << "Usage: crowbar-qt --publish <appid> [--id N] [--addon file.gma] [--icon image] [--changes text]\n";
				return 2;
			}
			QCoreApplication app(argc, argv);
			return publish(app, QString::fromLocal8Bit(argv[i + 1]).toUInt(), QCoreApplication::arguments().mid(i + 2));
		}
		if ((std::strcmp(argv[i], "--run-game") == 0 || std::strcmp(argv[i], "--hammer") == 0) && i + 2 < argc)
		{
			QCoreApplication app(argc, argv);
			const auto game = findApp(QString::fromLocal8Bit(argv[i + 1]).toUInt());
			if (!game)
				return 1;
			const QString gameDir = QFileInfo(QString::fromLocal8Bit(argv[i + 2])).absoluteFilePath();
			if (std::strcmp(argv[i], "--hammer") == 0)
				return run(app, runner::hammerCommand(*game, gameDir));
			const launcher::Command command = runner::gameCommand(*game, gameDir, QCoreApplication::arguments().mid(i + 3));
			if (!command.isValid())
			{
				QTextStream(stderr) << command.error << Qt::endl;
				return 1;
			}
			QTextStream(stdout) << command.displayString() << Qt::endl;
			return QProcess::startDetached(command.program, command.arguments) ? 0 : 1;
		}
		if (std::strcmp(argv[i], "--download") == 0 && i + 2 < argc)
		{
			QCoreApplication app(argc, argv);
			return download(app, QString::fromLocal8Bit(argv[i + 1]), QString::fromLocal8Bit(argv[i + 2]));
		}
		if (std::strcmp(argv[i], "--view") == 0)
		{
			if (i + 2 >= argc)
			{
				QTextStream(stderr) << "Usage: crowbar-qt --view <appid> <game dir> [mdl file]\n";
				return 2;
			}
			QCoreApplication app(argc, argv);
			return view(app, QString::fromLocal8Bit(argv[i + 1]).toUInt(), QString::fromLocal8Bit(argv[i + 2]),
						i + 3 < argc ? QString::fromLocal8Bit(argv[i + 3]) : QString());
		}
		if (std::strcmp(argv[i], "--compile") == 0)
		{
			if (i + 3 >= argc)
			{
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
	quitOnSignals();
	removeStaleTempDirs();
	// Lets Wayland compositors match the window to crowbar-qt.desktop and its icon.
	QGuiApplication::setDesktopFileName("crowbar-qt");
#ifdef CROWBAR_ICON
	QApplication::setWindowIcon(QIcon::fromTheme("crowbar-qt", QIcon(CROWBAR_ICON)));
#else
	QApplication::setWindowIcon(QIcon::fromTheme("crowbar-qt"));
#endif

	// The command-line modes above stay in English for scripts; only the window is translated.
	// The Settings page can pin a language; "en" means the untranslated strings.
	const QString language = QSettings().value("ui/language").toString();
	const QLocale locale = language.isEmpty() ? QLocale() : QLocale(language);
	if (!language.isEmpty())
		QLocale::setDefault(locale);
	QTranslator qtTranslator;
	QTranslator appTranslator;
	if (language != "en")
	{
		if (qtTranslator.load(locale, "qtbase", "_", QLibraryInfo::path(QLibraryInfo::TranslationsPath)))
			app.installTranslator(&qtTranslator);
		if (appTranslator.load(locale, "crowbar-qt", "_", ":/i18n"))
			app.installTranslator(&appTranslator);
	}

	MainWindow window;
	// Files given on the command line, e.g. from a file manager's "Open with".
	for (const QString &argument : QCoreApplication::arguments().mid(1))
	{
		if (!argument.startsWith(u'-'))
			window.openPath(QFileInfo(argument).absoluteFilePath());
	}
	window.show();

	return app.exec();
}
