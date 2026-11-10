#include "core/Publisher.h"

#include <QCoreApplication>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QImageReader>
#include <QImageWriter>
#include <QPainter>
#include <QRegularExpression>

namespace publisher {

namespace {

constexpr int IconSize = 512;

void setError(QString *error, const QString &text)
{
    if (error)
        *error = text;
}

launcher::Command failed(const QString &error)
{
    launcher::Command command;
    command.error = error;
    return command;
}

// Progressive JPEGs carry an SOF2 marker; gmpublish rejects them.
bool isBaselineJpeg(const QString &file)
{
    QFile in(file);
    if (!in.open(QIODevice::ReadOnly))
        return false;
    const QByteArray data = in.read(1 << 20);
    if (!data.startsWith("\xff\xd8"))
        return false;
    for (qsizetype i = 2; i + 3 < data.size();) {
        if (quint8(data[i]) != 0xff)
            return false;
        const quint8 marker = quint8(data[i + 1]);
        if (marker == 0xc0 || marker == 0xc1)
            return true;
        if (marker == 0xc2)
            return false;
        const int length = (quint8(data[i + 2]) << 8) | quint8(data[i + 3]);
        i += 2 + length;
    }
    return false;
}

} // namespace

QString findExecutable(const steam::App &app)
{
    return launcher::findTool(app, "gmpublish");
}

launcher::Command listCommand(const steam::App &app)
{
    const QString executable = findExecutable(app);
    if (executable.isEmpty())
        return failed(QCoreApplication::translate("Crowbar", "%1 has no gmpublish in its bin folder.").arg(app.name));
    launcher::Command command = launcher::build(app, executable, {"list"}, launcher::Mode::Console);
    command.workingDirectory = QFileInfo(executable).absolutePath();
    return command;
}

launcher::Command publishCommand(const steam::App &app, const Request &request)
{
    const QString executable = findExecutable(app);
    if (executable.isEmpty())
        return failed(QCoreApplication::translate("Crowbar", "%1 has no gmpublish in its bin folder.").arg(app.name));

    const bool create = request.id == 0;
    if (request.gmaFile.isEmpty() && (create || request.iconFile.isEmpty()))
        return failed(create ? QCoreApplication::translate("Crowbar", "A new item needs a GMA file and an icon.")
                             : QCoreApplication::translate("Crowbar", "Select a GMA file, an icon or both to update."));
    if (create && request.iconFile.isEmpty())
        return failed(QCoreApplication::translate("Crowbar", "A new item needs an icon."));
    for (const QString &file : {request.gmaFile, request.iconFile}) {
        if (!file.isEmpty() && !QFileInfo(file).isFile())
            return failed(QCoreApplication::translate("Crowbar", "\"%1\" does not exist.").arg(file));
    }

    const bool windows = launcher::isWindowsExecutable(executable);
    const auto path = [&](const QString &hostPath) {
        return windows ? launcher::toWindowsPath(hostPath) : QFileInfo(hostPath).absoluteFilePath();
    };

    QStringList arguments;
    if (create) {
        arguments << "create";
    } else {
        arguments << "update" << "-id" << QString::number(request.id);
    }
    if (!request.gmaFile.isEmpty())
        arguments << "-addon" << path(request.gmaFile);
    if (!request.iconFile.isEmpty())
        arguments << "-icon" << path(request.iconFile);
    if (!create && !request.changes.trimmed().isEmpty())
        arguments << "-changes" << request.changes.trimmed();

    launcher::Command command = launcher::build(app, executable, arguments, launcher::Mode::Console);
    command.workingDirectory = QFileInfo(executable).absolutePath();
    return command;
}

void ListParser::addLine(const QString &line)
{
    if (m_inTitle) {
        QString text = line;
        if (text.endsWith(u'"')) {
            text.chop(1);
            m_inTitle = false;
        }
        WorkshopItem &item = m_items.last();
        if (!text.trimmed().isEmpty())
            item.title += u' ' + text.trimmed();
        return;
    }

    static const QRegularExpression pattern(R"(^\t(\d+)\t([^\t]*)\t"(.*)$)");
    const auto match = pattern.match(line);
    if (!match.hasMatch())
        return;

    WorkshopItem item;
    item.id = match.captured(1).toULongLong();
    item.size = match.captured(2).trimmed();
    item.title = match.captured(3);
    if (item.title.endsWith(u'"'))
        item.title.chop(1);
    else
        m_inTitle = true;
    item.title = item.title.trimmed();
    m_items << item;
}

QList<WorkshopItem> ListParser::items() const
{
    return m_items;
}

quint64 parseCreatedId(const QString &line)
{
    static const QRegularExpression pattern("Your new item ID is (\\d+)");
    const auto match = pattern.match(line);
    return match.hasMatch() ? match.captured(1).toULongLong() : 0;
}

QString workshopUrl(quint64 id)
{
    return QStringLiteral("https://steamcommunity.com/sharedfiles/filedetails/?id=%1").arg(id);
}

QString prepareIcon(const QString &iconFile, const QString &convertedFile, QString *error)
{
    QImageReader reader(iconFile);
    const QSize size = reader.size();
    if (reader.format() == "jpeg" && size == QSize(IconSize, IconSize) && isBaselineJpeg(iconFile))
        return iconFile;

    QImage image = reader.read();
    if (image.isNull()) {
        setError(error, QCoreApplication::translate("Crowbar", "Could not read the icon \"%1\": %2").arg(iconFile, reader.errorString()));
        return {};
    }

    const int side = qMin(image.width(), image.height());
    image = image.copy((image.width() - side) / 2, (image.height() - side) / 2, side, side)
                .scaled(IconSize, IconSize, Qt::IgnoreAspectRatio, Qt::SmoothTransformation);
    // JPEG has no alpha; flatten onto white instead of the black Qt would use.
    QImage flat(image.size(), QImage::Format_RGB32);
    flat.fill(Qt::white);
    QPainter(&flat).drawImage(0, 0, image);

    QImageWriter writer(convertedFile, "jpeg");
    writer.setQuality(92);
    writer.setProgressiveScanWrite(false);
    if (!writer.write(flat)) {
        setError(error, QCoreApplication::translate("Crowbar", "Could not write \"%1\": %2").arg(convertedFile, writer.errorString()));
        return {};
    }
    return convertedFile;
}

} // namespace publisher
