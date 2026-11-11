#include "core/Launcher.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QProcess>
#include <QRegularExpression>

#include <algorithm>

namespace launcher
{

	QString Command::displayString() const
	{
		QStringList parts{program};
		parts += arguments;
		for (QString &part : parts)
		{
			if (part.isEmpty() || part.contains(u' '))
				part = u'"' + part + u'"';
		}
		return parts.join(u' ');
	}

	bool isWindowsExecutable(const QString &path)
	{
		QFile file(path);
		return file.open(QIODevice::ReadOnly) && file.read(2) == "MZ";
	}

	QString findTool(const steam::App &app, const QString &name)
	{
		if (app.toolsPath.isEmpty())
			return {};
		// 64-bit builds first: the 32-bit tools run out of address space on big
		// jobs, e.g. gmad on addons over 2 GB.
		for (const QString &dir : {QStringLiteral("/win64/"), QStringLiteral("/x64/"), QStringLiteral("/linux64/"), QStringLiteral("/")})
		{
			for (const QString &fileName : {name + ".exe", name})
			{
				const QString path = app.toolsPath + dir + fileName;
				if (QFileInfo(path).isFile())
					return path;
			}
		}
		return {};
	}

	QString findPathIgnoringCase(const QString &base, const QString &relative)
	{
		QString current = base.isEmpty() ? QStringLiteral("/") : base;
		for (const QString &part : relative.split(u'/', Qt::SkipEmptyParts))
		{
			const QDir dir(current);
			if (dir.exists(part))
			{
				current = dir.filePath(part);
				continue;
			}
			const QStringList entries = dir.entryList(QDir::AllEntries | QDir::NoDotAndDotDot | QDir::Hidden);
			const auto found = std::find_if(entries.begin(), entries.end(), [&](const QString &entry)
											{ return entry.compare(part, Qt::CaseInsensitive) == 0; });
			if (found == entries.end())
				return {};
			current = dir.filePath(*found);
		}
		return QFileInfo::exists(current) ? QDir::cleanPath(current) : QString();
	}

	QString toWindowsPath(const QString &hostPath)
	{
		QString path = QDir::cleanPath(QFileInfo(hostPath).absoluteFilePath());
		return "Z:" + path.replace(u'/', u'\\');
	}

	QString toHostPaths(QString text)
	{
		static const QRegularExpression pattern(R"([Zz]:[\\/][^"\r\n:*?<>|]*)");
		QList<QRegularExpressionMatch> matches;
		auto it = pattern.globalMatch(text);
		while (it.hasNext())
			matches << it.next();
		for (auto match = matches.crbegin(); match != matches.crend(); ++match)
		{
			QString path = match->captured().mid(2);
			path.replace(u'\\', u'/');
			text.replace(match->capturedStart(), match->capturedLength(), path);
		}
		return text;
	}

	QString stripTerminalCodes(QString text)
	{
		static const QRegularExpression codes("\\x1b\\[[0-9;?]*[A-Za-z]|\\x1b[()][A-Za-z0-9]|\\x1b[=>]");
		return text.remove(codes);
	}

	bool isProtonNoise(QStringView line)
	{
		return line.startsWith(u"ProtonFixes[") || line.startsWith(u"ntsync:") || line.startsWith(u"fsync:") || line.startsWith(u"esync:");
	}

	namespace
	{

		QStringList &toolEnvironment()
		{
			static QStringList lines;
			return lines;
		}

	} // namespace

	void setToolEnvironment(const QStringList &lines)
	{
		toolEnvironment() = lines;
	}

	QStringList applyEnvironmentLines(QProcessEnvironment &environment, const QStringList &lines)
	{
		static const QRegularExpression name("^[A-Za-z_][A-Za-z0-9_]*$");
		QStringList invalid;
		for (const QString &raw : lines)
		{
			const QString line = raw.trimmed();
			if (line.isEmpty() || line.startsWith(u'#'))
				continue;
			if (line.startsWith(u'-') && name.match(line.mid(1)).hasMatch())
			{
				environment.remove(line.mid(1));
				continue;
			}
			const qsizetype equals = line.indexOf(u'=');
			if (equals > 0 && name.match(line.left(equals)).hasMatch())
			{
				environment.insert(line.left(equals), line.mid(equals + 1));
				continue;
			}
			invalid << raw;
		}
		return invalid;
	}

	Command build(const steam::App &app, const QString &executable, const QStringList &arguments, Mode mode)
	{
		Command command;
		command.environment = QProcessEnvironment::systemEnvironment();
		// The user's own settings first; the Proton variables below must win over them.
		applyEnvironmentLines(command.environment, toolEnvironment());

		if (!QFile::exists(executable))
		{
			command.error = QCoreApplication::translate("Crowbar", "\"%1\" does not exist.").arg(executable);
			return command;
		}

		if (!isWindowsExecutable(executable))
		{
			command.program = executable;
			command.arguments = arguments;
			return command;
		}

		if (app.protonPath.isEmpty())
		{
			command.error = QCoreApplication::translate("Crowbar", "No Proton version found for %1.").arg(app.name);
			return command;
		}
		if (app.compatDataPath.isEmpty())
		{
			command.error = QCoreApplication::translate("Crowbar", "%1 has no Proton prefix yet. Start the game once from Steam.").arg(app.name);
			return command;
		}

		// "run" goes through Proton's steam.exe shim, which gives the program a
		// working Steam API but drops its stdout. "runinprefix" skips the shim.
		command.program = app.protonPath + "/proton";
		command.arguments = {mode == Mode::Console ? "runinprefix" : "run", executable};
		command.arguments += arguments;

		const QString appId = QString::number(app.id);
		command.environment.insert("STEAM_COMPAT_DATA_PATH", app.compatDataPath);
		command.environment.insert("STEAM_COMPAT_CLIENT_INSTALL_PATH", app.steamRoot);
		command.environment.insert("SteamAppId", appId);
		command.environment.insert("SteamGameId", appId);
		// Under Wine's Wayland driver the menus of these old Win32 tools close as
		// soon as they open, so a global PROTON_ENABLE_WAYLAND=1 must not reach them.
		command.environment.remove("PROTON_ENABLE_WAYLAND");
		return command;
	}

} // namespace launcher
