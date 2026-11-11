#include "core/Studiomdl.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QRegularExpression>

#include <algorithm>

namespace studiomdl
{

	QString findExecutable(const steam::App &app)
	{
		return launcher::findTool(app, "studiomdl");
	}

	launcher::Command compileCommand(const steam::App &app, const QString &gameDir,
									 const QString &qcFile, const Options &options)
	{
		const QString executable = findExecutable(app);
		if (executable.isEmpty())
		{
			launcher::Command command;
			command.error = QCoreApplication::translate("Crowbar", "%1 has no studiomdl in its bin folder.").arg(app.name);
			return command;
		}

		const QFileInfo qc(qcFile);
		if (!qc.isFile())
		{
			launcher::Command command;
			command.error = QCoreApplication::translate("Crowbar", "\"%1\" does not exist.").arg(qcFile);
			return command;
		}

		const bool windows = launcher::isWindowsExecutable(executable);
		QStringList arguments{"-game", windows ? launcher::toWindowsPath(gameDir) : gameDir};
		if (options.noP4)
			arguments << "-nop4";
		if (options.verbose)
			arguments << "-verbose";
		arguments += options.extra;
		// Relative name plus working directory, as studiomdl resolves $include and SMD paths from there.
		arguments << qc.fileName();

		launcher::Command command = launcher::build(app, executable, arguments, launcher::Mode::Console);
		command.workingDirectory = qc.absolutePath();
		return command;
	}

	QString readModelName(const QString &qcFile)
	{
		QFile file(qcFile);
		if (!file.open(QIODevice::ReadOnly))
			return {};

		static const QRegularExpression pattern(
			R"(^\s*\$modelname\s+(?:"([^"]*)\"|(\S+)))",
			QRegularExpression::CaseInsensitiveOption | QRegularExpression::MultilineOption);
		const auto match = pattern.match(QString::fromUtf8(file.readAll()));
		if (!match.hasMatch())
			return {};

		QString name = match.captured(1).isEmpty() ? match.captured(2) : match.captured(1);
		name.replace(u'\\', u'/');
		name = QDir::cleanPath(name);
		if (!name.endsWith(".mdl", Qt::CaseInsensitive))
			name += ".mdl";
		return name;
	}

	bool moveCompiledModel(const QString &gameDir, const QString &modelName,
						   const QString &outputDir, QStringList &log)
	{
		const QFileInfo model(modelName);
		const QString subpath = model.path() == "." ? QString() : model.path();
		const QString sourceDir = launcher::findPathIgnoringCase(gameDir, "models/" + subpath);
		if (sourceDir.isEmpty())
		{
			log << QCoreApplication::translate("Crowbar", "WARNING: Compiled model folder \"%1/models/%2\" was not found.").arg(gameDir, subpath);
			return false;
		}

		const QString targetDir = QDir::cleanPath(outputDir + "/models/" + subpath);
		if (QFileInfo(targetDir) == QFileInfo(sourceDir))
			return true;
		if (!QDir().mkpath(targetDir))
		{
			log << QCoreApplication::translate("Crowbar", "WARNING: Could not create \"%1\".").arg(targetDir);
			return false;
		}

		static const QStringList extensions{"ani", "mdl", "phy", "vtx", "vvd"};
		const QString prefix = model.completeBaseName() + '.';
		int moved = 0;
		for (const QFileInfo &file : QDir(sourceDir).entryInfoList(QDir::Files))
		{
			if (!file.fileName().startsWith(prefix, Qt::CaseInsensitive) || !extensions.contains(file.suffix().toLower()))
				continue;
			const QString target = targetDir + '/' + file.fileName();
			QFile::remove(target);
			if (QFile::rename(file.absoluteFilePath(), target))
			{
				++moved;
				log << QCoreApplication::translate("Crowbar", "Moved \"%1\" to \"%2\".").arg(file.fileName(), targetDir);
			}
			else
			{
				log << QCoreApplication::translate("Crowbar", "WARNING: Could not move \"%1\" to \"%2\".").arg(file.absoluteFilePath(), targetDir);
			}
		}

		if (moved == 0)
			log << QCoreApplication::translate("Crowbar", "WARNING: No compiled files for \"%1\" found in \"%2\".").arg(modelName, sourceDir);
		// Drops the subfolders the move emptied, but never "models" itself.
		QString emptied = sourceDir;
		for (qsizetype i = subpath.split(u'/', Qt::SkipEmptyParts).size(); i > 0 && QDir().rmdir(emptied); --i)
			emptied = QFileInfo(emptied).absolutePath();
		return moved > 0;
	}

} // namespace studiomdl
