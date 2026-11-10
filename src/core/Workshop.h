#pragma once

#include "core/Launcher.h"

#include <QDateTime>
#include <QObject>

class QNetworkAccessManager;
class QNetworkReply;

namespace workshop {

struct ItemDetails {
    quint64 id = 0;
    quint32 appId = 0;
    QString title;
    QString fileName;
    QString fileUrl; // only old items have a direct link
    qint64 fileSize = 0;
    QDateTime updated;
    QString error;

    bool isValid() const { return error.isEmpty() && id != 0; }
};

// Accepts a bare ID or any workshop/community URL with ?id=. Returns 0 if none.
quint64 parseItemId(const QString &text);

QString itemUrl(quint64 id);

// Parses the response of ISteamRemoteStorage/GetPublishedFileDetails.
ItemDetails parseDetails(const QByteArray &json);

// Files Steam already downloaded for a subscribed item, from every library.
QStringList findCachedFiles(const QStringList &libraries, quint32 appId, quint64 id);

QString findSteamCmd();
launcher::Command steamCmdCommand(const QString &steamCmd, const QString &installDir, quint32 appId, quint64 id);
// Folder from steamcmd's "Success. Downloaded item ... to "<path>"" line, or empty.
QString parseSteamCmdFolder(const QString &line);

// Copies a downloaded file into outputDir as <baseName>.<ext>. LZMA-compressed
// files are unpacked, and become .gma when they hold an addon.
QString saveFile(const QString &file, const QString &outputDir, const QString &baseName, bool convert,
                 QString *error = nullptr);

// Safe file name from an item title: "<title> (<id>)".
QString fileBaseName(const ItemDetails &details, bool withTitle);

class Client : public QObject {
    Q_OBJECT

public:
    explicit Client(QObject *parent = nullptr);

    void fetchDetails(quint64 id);
    void download(const QString &url, const QString &file);
    void abort();

signals:
    void detailsReady(const workshop::ItemDetails &details);
    void progress(qint64 received, qint64 total);
    void downloaded(bool ok, const QString &error);

private:
    QNetworkAccessManager *m_network;
    QNetworkReply *m_reply = nullptr;
};

} // namespace workshop
