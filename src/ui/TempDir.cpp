#include "ui/TempDir.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QRegularExpression>
#include <QTemporaryDir>

#include <memory>

namespace
{

	// The process ID in the name tells a later run whether the owner is gone.
	QString namePrefix()
	{
		return QStringLiteral("crowbar-qt-%1-").arg(QCoreApplication::applicationPid());
	}

	std::unique_ptr<QTemporaryDir> &holder()
	{
		static std::unique_ptr<QTemporaryDir> dir;
		return dir;
	}

} // namespace

QString tempPath(const QString &subfolder)
{
	std::unique_ptr<QTemporaryDir> &dir = holder();
	if (!dir)
	{
		dir = std::make_unique<QTemporaryDir>(QDir::tempPath() + u'/' + namePrefix() + "XXXXXX");
		if (!dir->isValid())
		{
			dir.reset();
			return {};
		}
		// Removed before QCoreApplication goes away, so a quit by signal cleans up too.
		qAddPostRoutine([]
						{ holder().reset(); });
	}
	if (subfolder.isEmpty())
		return dir->path();
	const QString path = dir->filePath(subfolder);
	QDir().mkpath(path);
	return path;
}

void removeStaleTempDirs()
{
	static const QRegularExpression owned("^crowbar-qt-(\\d+)-\\w{6}$");
	static const QRegularExpression legacy("^crowbar-qt-\\w{6}$");
	const QDir temp(QDir::tempPath());
	for (const QFileInfo &entry : temp.entryInfoList({"crowbar-qt-*"}, QDir::Dirs | QDir::NoDotAndDotDot))
	{
		QDir dir(entry.absoluteFilePath());
		const auto match = owned.match(entry.fileName());
		if (match.hasMatch())
		{
			const qint64 pid = match.captured(1).toLongLong();
			if (pid != QCoreApplication::applicationPid() && !QFileInfo::exists(QStringLiteral("/proc/%1").arg(pid)))
				dir.removeRecursively();
		}
		else if (legacy.match(entry.fileName()).hasMatch() && dir.isEmpty())
		{
			// Older versions did not record their process; only their empty folders are safe to drop.
			dir.removeRecursively();
		}
	}
}
