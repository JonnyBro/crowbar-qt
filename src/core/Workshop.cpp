#include "core/Workshop.h"

#include "core/Package.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QRegularExpression>
#include <QSaveFile>
#include <QStandardPaths>
#include <QUrlQuery>

namespace workshop {

quint64 parseItemId(const QString &text)
{
    const QString trimmed = text.trimmed();
    bool ok = false;
    const quint64 id = trimmed.toULongLong(&ok);
    if (ok)
        return id;
    static const QRegularExpression pattern("[?&]id=(\\d+)");
    const auto match = pattern.match(trimmed);
    return match.hasMatch() ? match.captured(1).toULongLong() : 0;
}

QString itemUrl(quint64 id)
{
    return QStringLiteral("https://steamcommunity.com/sharedfiles/filedetails/?id=%1").arg(id);
}

ItemDetails parseDetails(const QByteArray &json)
{
    ItemDetails details;
    const QJsonArray items = QJsonDocument::fromJson(json).object()["response"].toObject()["publishedfiledetails"].toArray();
    if (items.isEmpty()) {
        details.error = QCoreApplication::translate("Crowbar", "Steam returned no item details.");
        return details;
    }
    const QJsonObject item = items.first().toObject();
    details.id = item["publishedfileid"].toString().toULongLong();
    if (item["result"].toInt() != 1) {
        details.error = QCoreApplication::translate("Crowbar", "Item %1 was not found, or it is private or removed.").arg(details.id);
        return details;
    }
    details.appId = quint32(item["consumer_app_id"].toInteger());
    details.title = item["title"].toString();
    details.fileName = item["filename"].toString();
    details.fileUrl = item["file_url"].toString();
    details.fileSize = item["file_size"].toVariant().toLongLong();
    details.updated = QDateTime::fromSecsSinceEpoch(item["time_updated"].toInteger());
    return details;
}

QStringList findCachedFiles(const QStringList &libraries, quint32 appId, quint64 id)
{
    for (const QString &library : libraries) {
        const QDir dir(QStringLiteral("%1/steamapps/workshop/content/%2/%3").arg(library).arg(appId).arg(id));
        QStringList files;
        for (const QFileInfo &file : dir.entryInfoList(QDir::Files, QDir::Name))
            files << file.absoluteFilePath();
        if (!files.isEmpty())
            return files;
    }
    return {};
}

QString findSteamCmd()
{
    QString path = QStandardPaths::findExecutable("steamcmd");
    if (path.isEmpty()) {
        for (const QString &candidate : {QDir::homePath() + "/.steam/steamcmd/steamcmd.sh",
                                         QDir::homePath() + "/steamcmd/steamcmd.sh"}) {
            if (QFileInfo(candidate).isExecutable())
                return candidate;
        }
    }
    return path;
}

launcher::Command steamCmdCommand(const QString &steamCmd, const QString &installDir, quint32 appId, quint64 id)
{
    launcher::Command command;
    command.environment = QProcessEnvironment::systemEnvironment();
    if (steamCmd.isEmpty()) {
        command.error = QCoreApplication::translate("Crowbar", "steamcmd is not installed.");
        return command;
    }
    command.program = steamCmd;
    command.arguments = {"+force_install_dir", installDir, "+login", "anonymous",
                         "+workshop_download_item", QString::number(appId), QString::number(id), "+quit"};
    return command;
}

QString parseSteamCmdFolder(const QString &line)
{
    static const QRegularExpression pattern("Success\\. Downloaded item \\d+ to \"([^\"]+)\"");
    const auto match = pattern.match(line);
    return match.hasMatch() ? match.captured(1) : QString();
}

QString fileBaseName(const ItemDetails &details, bool withTitle)
{
    if (!withTitle || details.title.trimmed().isEmpty())
        return QString::number(details.id);
    QString title = details.title.simplified();
    static const QRegularExpression unsafe("[/\\\\:*?\"<>|\\x00-\\x1f]");
    title.replace(unsafe, "_");
    return QStringLiteral("%1 (%2)").arg(title.left(120).trimmed()).arg(details.id);
}

QString saveFile(const QString &file, const QString &outputDir, const QString &baseName, bool convert, QString *error)
{
    if (!QDir().mkpath(outputDir)) {
        if (error)
            *error = QCoreApplication::translate("Crowbar", "Could not create \"%1\".").arg(outputDir);
        return {};
    }

    if (convert && package::isLzmaFile(file)) {
        const QString temp = outputDir + "/." + baseName + ".partial";
        if (!package::decompressLzmaFile(file, temp, error))
            return {};
        QFile probe(temp);
        const bool gma = probe.open(QIODevice::ReadOnly) && probe.read(4) == "GMAD";
        probe.close();
        const QString target = outputDir + u'/' + baseName + (gma ? ".gma" : ".bin");
        QFile::remove(target);
        if (!QFile::rename(temp, target)) {
            QFile::remove(temp);
            if (error)
                *error = QCoreApplication::translate("Crowbar", "Could not write \"%1\".").arg(target);
            return {};
        }
        return target;
    }

    const QString suffix = QFileInfo(file).suffix();
    const QString target = outputDir + u'/' + baseName + (suffix.isEmpty() ? QString() : u'.' + suffix);
    if (QFileInfo(file) == QFileInfo(target))
        return target;
    QFile::remove(target);
    if (!QFile::copy(file, target)) {
        if (error)
            *error = QCoreApplication::translate("Crowbar", "Could not copy \"%1\" to \"%2\".").arg(file, target);
        return {};
    }
    return target;
}

Client::Client(QObject *parent)
    : QObject(parent)
    , m_network(new QNetworkAccessManager(this))
{
}

void Client::fetchDetails(quint64 id)
{
    QNetworkRequest request(QUrl("https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/"));
    request.setHeader(QNetworkRequest::ContentTypeHeader, "application/x-www-form-urlencoded");
    QUrlQuery form;
    form.addQueryItem("itemcount", "1");
    form.addQueryItem("publishedfileids[0]", QString::number(id));
    QNetworkReply *reply = m_network->post(request, form.toString(QUrl::FullyEncoded).toUtf8());
    connect(reply, &QNetworkReply::finished, this, [this, reply, id] {
        reply->deleteLater();
        ItemDetails details;
        if (reply->error() != QNetworkReply::NoError) {
            details.id = id;
            details.error = QCoreApplication::translate("Crowbar", "Could not reach Steam: %1").arg(reply->errorString());
        } else {
            details = parseDetails(reply->readAll());
        }
        emit detailsReady(details);
    });
}

void Client::download(const QString &url, const QString &file)
{
    auto *out = new QSaveFile(file, this);
    if (!out->open(QIODevice::WriteOnly)) {
        const QString error = out->errorString();
        delete out;
        emit downloaded(false, error);
        return;
    }
    m_reply = m_network->get(QNetworkRequest(QUrl(url)));
    connect(m_reply, &QNetworkReply::downloadProgress, this, &Client::progress);
    connect(m_reply, &QNetworkReply::readyRead, this, [this, out] { out->write(m_reply->readAll()); });
    connect(m_reply, &QNetworkReply::finished, this, [this, out] {
        QNetworkReply *reply = m_reply;
        m_reply = nullptr;
        reply->deleteLater();
        out->write(reply->readAll());
        const bool ok = reply->error() == QNetworkReply::NoError && out->commit();
        const QString error = reply->error() != QNetworkReply::NoError ? reply->errorString() : out->errorString();
        if (!ok)
            out->cancelWriting();
        out->deleteLater();
        emit downloaded(ok, ok ? QString() : error);
    });
}

void Client::abort()
{
    if (m_reply)
        m_reply->abort();
}

} // namespace workshop
