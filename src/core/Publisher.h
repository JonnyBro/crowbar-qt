#pragma once

#include "core/Launcher.h"

#include <QList>
#include <QString>

namespace publisher
{

	struct WorkshopItem
	{
		quint64 id = 0;
		QString size;
		QString title;
	};

	struct Request
	{
		quint64 id = 0; // 0 creates a new item
		QString gmaFile;
		QString iconFile;
		QString changes;
	};

	QString findExecutable(const steam::App &app);

	launcher::Command listCommand(const steam::App &app);
	launcher::Command publishCommand(const steam::App &app, const Request &request);

	// Collects items from `gmpublish list` output. Titles can contain line
	// breaks, so lines are fed one by one and an item may span several of them.
	class ListParser
	{
	public:
		void addLine(const QString &line);
		QList<WorkshopItem> items() const;

	private:
		QList<WorkshopItem> m_items;
		bool m_inTitle = false;
	};

	// ID printed by `gmpublish create`, or 0.
	quint64 parseCreatedId(const QString &line);

	QString workshopUrl(quint64 id);

	// gmpublish only takes a baseline 512x512 JPEG. Anything else Qt can read is
	// cropped to a square, scaled and written to `convertedFile`. Returns the file
	// to pass to gmpublish, or an empty string with `error` set.
	QString prepareIcon(const QString &iconFile, const QString &convertedFile, QString *error = nullptr);

} // namespace publisher
