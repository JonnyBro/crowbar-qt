#pragma once

#include "core/Launcher.h"

#include <QDateTime>
#include <QObject>

class QNetworkAccessManager;
class QNetworkReply;

namespace workshop
{

	struct ItemDetails
	{
		quint64 id = 0;
		quint32 appId = 0;
		QString title;
		QString fileName;
		QString fileUrl; // only old items have a direct link
		QString previewUrl;
		qint64 fileSize = 0;
		QDateTime updated;
		QString error;

		bool isValid() const { return error.isEmpty() && id != 0; }
	};

	// Accepts a bare ID or any workshop/community URL with ?id=. Returns 0 if none.
	quint64 parseItemId(const QString &text);
	// Every item ID in text: links and bare IDs separated by spaces, commas or lines.
	QList<quint64> parseItemIds(const QString &text);

	// Items of a collection from ISteamRemoteStorage/GetCollectionDetails; sets
	// isCollection to false when the ID is an ordinary item.
	QList<quint64> parseCollection(const QByteArray &json, bool *isCollection);

	QString itemUrl(quint64 id);

	// Parses the response of ISteamRemoteStorage/GetPublishedFileDetails.
	ItemDetails parseDetails(const QByteArray &json);
	// Every item in the response, in order; failed ones carry an error.
	QList<ItemDetails> parseDetailsList(const QByteArray &json);

	// Files Steam already downloaded for a subscribed item, from every library.
	QStringList findCachedFiles(const QStringList &libraries, quint32 appId, quint64 id);

	QString findSteamCmd();
	// steamcmd is a cut-down Steam client: given the real HOME it treats ~/.steam
	// as its own install and rewrites the Steam client's files there, e.g. the list
	// of libraries. homeDir is a folder of its own it runs in instead.
	launcher::Command steamCmdCommand(const QString &steamCmd, const QString &homeDir, const QString &installDir,
									  quint32 appId, quint64 id);
	// Path from steamcmd's "Success. Downloaded item ... to "<path>"" line, or
	// empty. Depending on the item it names the downloaded file or its folder.
	QString parseSteamCmdPath(const QString &line);

	// Copies a downloaded file into outputDir as <baseName>.<ext>. LZMA-compressed
	// files are unpacked, and become .gma when they hold an addon.
	QString saveFile(const QString &file, const QString &outputDir, const QString &baseName, bool convert,
					 QString *error = nullptr);

	// Safe file name from an item title: "<title> (<id>)".
	QString fileBaseName(const ItemDetails &details, bool withTitle);

	class Client : public QObject
	{
		Q_OBJECT

	public:
		explicit Client(QObject *parent = nullptr);

		void fetchDetails(quint64 id);
		void fetchDetailsList(const QList<quint64> &ids);
		void fetchCollection(quint64 id);
		// Small downloads kept in memory, e.g. preview images.
		void fetchData(const QString &url, quint64 id);
		void download(const QString &url, const QString &file);
		void abort();

	signals:
		void detailsReady(const workshop::ItemDetails &details);
		void detailsListReady(const QList<workshop::ItemDetails> &details);
		// A failed lookup reports the ID as an ordinary item.
		void collectionReady(quint64 id, const QList<quint64> &children, bool isCollection);
		void dataReady(quint64 id, const QByteArray &data);
		void progress(qint64 received, qint64 total);
		void downloaded(bool ok, const QString &error);

	private:
		QNetworkAccessManager *m_network;
		QNetworkReply *m_reply = nullptr;
	};

} // namespace workshop
