#include "core/Package.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QRegularExpression>
#include <QTemporaryFile>
#include <QtEndian>

#include <lzma.h>

namespace package {

namespace {

constexpr quint32 VpkSignature = 0x55aa1234;
constexpr quint16 VpkEntryTerminator = 0xffff;
constexpr quint16 VpkDirArchiveIndex = 0x7fff;

void setError(QString *error, const QString &text)
{
    if (error)
        *error = text;
}

// Sequential little-endian reader over a byte buffer.
class Reader {
public:
    explicit Reader(const QByteArray &data) : m_data(data) {}

    bool ok() const { return m_ok; }
    qsizetype position() const { return m_pos; }

    template <typename T>
    T read()
    {
        if (m_pos + qsizetype(sizeof(T)) > m_data.size()) {
            m_ok = false;
            return {};
        }
        const T value = qFromLittleEndian<T>(m_data.constData() + m_pos);
        m_pos += sizeof(T);
        return value;
    }

    QByteArray bytes(qsizetype count)
    {
        if (count < 0 || m_pos + count > m_data.size()) {
            m_ok = false;
            return {};
        }
        const QByteArray result = m_data.mid(m_pos, count);
        m_pos += count;
        return result;
    }

    QString string()
    {
        const qsizetype end = m_data.indexOf('\0', m_pos);
        if (end < 0) {
            m_ok = false;
            return {};
        }
        const QString result = QString::fromUtf8(m_data.constData() + m_pos, end - m_pos);
        m_pos = end + 1;
        return result;
    }

private:
    const QByteArray &m_data;
    qsizetype m_pos = 0;
    bool m_ok = true;
};

// Package paths come from untrusted files; anything that would land outside
// the output folder is rejected.
QString safeRelativePath(QString path)
{
    path.replace(u'\\', u'/');
    path = QDir::cleanPath(path);
    if (path.isEmpty() || path == "." || path == ".." || path.startsWith("../")
        || QDir::isAbsolutePath(path) || path.contains(u':'))
        return {};
    return path;
}

// Legacy ".lzma" container: 0x5d properties byte, then a little-endian dictionary size.
bool looksLikeLzma(const QByteArray &head)
{
    return head.size() >= 13 && head[0] == 0x5d && quint8(head[4]) <= 0x10;
}

std::shared_ptr<QTemporaryFile> decompressLzma(const QString &file, QString *error)
{
    QFile in(file);
    auto out = std::make_shared<QTemporaryFile>();
    if (!in.open(QIODevice::ReadOnly) || !out->open()) {
        setError(error, QCoreApplication::translate("Crowbar", "Could not open \"%1\" for decompression.").arg(file));
        return nullptr;
    }

    lzma_stream stream = LZMA_STREAM_INIT;
    if (lzma_alone_decoder(&stream, UINT64_MAX) != LZMA_OK) {
        setError(error, QCoreApplication::translate("Crowbar", "Could not start the LZMA decoder."));
        return nullptr;
    }

    QByteArray inBuffer;
    QByteArray outBuffer(1 << 20, Qt::Uninitialized);
    lzma_ret result = LZMA_OK;
    while (result == LZMA_OK) {
        if (stream.avail_in == 0 && !in.atEnd()) {
            inBuffer = in.read(1 << 20);
            stream.next_in = reinterpret_cast<const uint8_t *>(inBuffer.constData());
            stream.avail_in = size_t(inBuffer.size());
        }
        stream.next_out = reinterpret_cast<uint8_t *>(outBuffer.data());
        stream.avail_out = size_t(outBuffer.size());
        result = lzma_code(&stream, in.atEnd() ? LZMA_FINISH : LZMA_RUN);
        const qint64 produced = outBuffer.size() - qint64(stream.avail_out);
        if (out->write(outBuffer.constData(), produced) != produced)
            result = LZMA_PROG_ERROR;
    }
    lzma_end(&stream);

    if (result != LZMA_STREAM_END) {
        setError(error, QCoreApplication::translate("Crowbar", "\"%1\" is not a valid LZMA-compressed package.").arg(QFileInfo(file).fileName()));
        return nullptr;
    }
    out->flush();
    return out;
}

} // namespace

std::optional<Package> readVpk(const QString &file, QString *error)
{
    QFile dirFile(file);
    if (!dirFile.open(QIODevice::ReadOnly)) {
        setError(error, dirFile.errorString());
        return std::nullopt;
    }

    const QByteArray head = dirFile.read(28);
    Reader header(head);
    if (header.read<quint32>() != VpkSignature) {
        setError(error, QCoreApplication::translate("Crowbar", "Not a VPK directory file. Open the *_dir.vpk file of a multi-part package."));
        return std::nullopt;
    }
    const quint32 version = header.read<quint32>();
    const quint32 treeSize = header.read<quint32>();
    if (!header.ok() || (version != 1 && version != 2)) {
        setError(error, QCoreApplication::translate("Crowbar", "Unsupported VPK version %1.").arg(version));
        return std::nullopt;
    }
    const qint64 headerSize = version == 1 ? 12 : 28;
    if (!dirFile.seek(headerSize)) {
        setError(error, dirFile.errorString());
        return std::nullopt;
    }
    const QByteArray treeData = dirFile.read(treeSize);
    if (quint32(treeData.size()) != treeSize) {
        setError(error, QCoreApplication::translate("Crowbar", "The VPK file table is truncated."));
        return std::nullopt;
    }

    const QFileInfo info(file);
    QString base = info.completeBaseName();
    if (base.endsWith("_dir", Qt::CaseInsensitive))
        base.chop(4);
    const quint64 embeddedDataStart = quint64(headerSize) + treeSize;

    Package result;
    result.m_file = file;
    result.m_format = QStringLiteral("VPK v%1").arg(version);

    // A single space stands for "empty" in all three levels of the tree.
    const auto blank = [](const QString &text) { return text == " " ? QString() : text; };

    Reader tree(treeData);
    for (;;) {
        const QString extension = tree.string();
        if (!tree.ok() || extension.isEmpty())
            break;
        for (;;) {
            const QString directory = tree.string();
            if (!tree.ok() || directory.isEmpty())
                break;
            for (;;) {
                const QString name = tree.string();
                if (!tree.ok() || name.isEmpty())
                    break;

                Entry entry;
                entry.crc = tree.read<quint32>();
                const quint16 preloadSize = tree.read<quint16>();
                const quint16 archiveIndex = tree.read<quint16>();
                entry.offset = tree.read<quint32>();
                entry.length = tree.read<quint32>();
                const quint16 terminator = tree.read<quint16>();
                entry.preload = tree.bytes(preloadSize);
                if (!tree.ok() || terminator != VpkEntryTerminator) {
                    setError(error, QCoreApplication::translate("Crowbar", "The VPK file table is corrupt."));
                    return std::nullopt;
                }

                entry.path = blank(directory);
                if (!entry.path.isEmpty())
                    entry.path += u'/';
                entry.path += blank(name);
                if (!blank(extension).isEmpty())
                    entry.path += u'.' + extension;
                entry.size = entry.length + preloadSize;

                if (archiveIndex == VpkDirArchiveIndex) {
                    entry.archive = file;
                    entry.offset += embeddedDataStart;
                } else {
                    entry.archive = info.absolutePath()
                        + QStringLiteral("/%1_%2.vpk").arg(base).arg(archiveIndex, 3, 10, QChar(u'0'));
                }
                result.m_entries << entry;
            }
        }
    }
    if (!tree.ok()) {
        setError(error, QCoreApplication::translate("Crowbar", "The VPK file table is corrupt."));
        return std::nullopt;
    }
    return result;
}

std::optional<Package> readGma(const QString &file, QString *error)
{
    QFile gma(file);
    if (!gma.open(QIODevice::ReadOnly)) {
        setError(error, gma.errorString());
        return std::nullopt;
    }

    // The file table sits at the start and is small next to the data, but its
    // size is not stored anywhere, so read in growing chunks until it parses.
    for (qint64 chunk = 1 << 20;; chunk *= 8) {
        gma.seek(0);
        const QByteArray data = gma.read(chunk);
        const bool wholeFile = data.size() == gma.size();

        Reader reader(data);
        if (reader.bytes(4) != "GMAD") {
            setError(error, QCoreApplication::translate("Crowbar", "Not a GMA file."));
            return std::nullopt;
        }
        const quint8 version = reader.read<quint8>();
        reader.read<quint64>(); // author SteamID
        reader.read<quint64>(); // timestamp
        if (version > 1) {
            while (reader.ok() && !reader.string().isEmpty()) {
                // required content, unused by the game
            }
        }

        Package result;
        result.m_file = file;
        result.m_format = QStringLiteral("GMA v%1").arg(version);
        result.m_title = reader.string();
        reader.string(); // description
        reader.string(); // author
        reader.read<qint32>(); // addon version

        quint64 dataOffset = 0;
        while (reader.ok()) {
            if (reader.read<quint32>() == 0)
                break;
            Entry entry;
            entry.path = reader.string();
            entry.size = entry.length = quint64(reader.read<qint64>());
            entry.crc = reader.read<quint32>();
            entry.archive = file;
            entry.offset = dataOffset;
            dataOffset += entry.length;
            result.m_entries << entry;
        }

        if (reader.ok()) {
            for (Entry &entry : result.m_entries)
                entry.offset += quint64(reader.position());
            return result;
        }
        if (wholeFile) {
            setError(error, QCoreApplication::translate("Crowbar", "The GMA file table is corrupt."));
            return std::nullopt;
        }
    }
}

std::optional<Package> Package::open(const QString &file, QString *error)
{
    QFile probe(file);
    if (!probe.open(QIODevice::ReadOnly)) {
        setError(error, probe.errorString());
        return std::nullopt;
    }
    const QByteArray head = probe.read(13);
    const QByteArray magic = head.left(4);
    probe.close();

    if (magic == "GMAD")
        return readGma(file, error);
    if (magic.size() == 4 && qFromLittleEndian<quint32>(magic.constData()) == VpkSignature)
        return readVpk(file, error);

    if (looksLikeLzma(head)) {
        const auto decompressed = decompressLzma(file, error);
        if (!decompressed)
            return std::nullopt;
        auto package = readGma(decompressed->fileName(), nullptr);
        if (!package) {
            setError(error, QCoreApplication::translate("Crowbar", "\"%1\" is LZMA-compressed but holds no GMA addon (it may be a dupe or a save).")
                                .arg(QFileInfo(file).fileName()));
        } else {
            package->m_file = file;
            package->m_format += " (LZMA)";
            package->m_decompressed = decompressed;
        }
        return package;
    }

    static const QRegularExpression numberedPart("_\\d{3}\\.vpk$", QRegularExpression::CaseInsensitiveOption);
    const auto match = numberedPart.match(file);
    if (match.hasMatch()) {
        const QString dirFile = file.left(match.capturedStart()) + "_dir.vpk";
        if (QFile::exists(dirFile))
            return readVpk(dirFile, error);
    }
    setError(error, QCoreApplication::translate("Crowbar", "\"%1\" is not a VPK or GMA package.").arg(QFileInfo(file).fileName()));
    return std::nullopt;
}

bool isLzmaFile(const QString &file)
{
    QFile in(file);
    return in.open(QIODevice::ReadOnly) && looksLikeLzma(in.read(13));
}

bool decompressLzmaFile(const QString &file, const QString &outputFile, QString *error)
{
    const auto decompressed = decompressLzma(file, error);
    if (!decompressed)
        return false;
    QFile::remove(outputFile);
    if (!QFile::copy(decompressed->fileName(), outputFile)) {
        setError(error, QCoreApplication::translate("Crowbar", "Could not write \"%1\".").arg(outputFile));
        return false;
    }
    return true;
}

bool Package::extract(const Entry &entry, const QString &outputDir, QString *error) const
{
    const QString relative = safeRelativePath(entry.path);
    if (relative.isEmpty()) {
        setError(error, QCoreApplication::translate("Crowbar", "Unsafe path \"%1\" skipped.").arg(entry.path));
        return false;
    }

    const QString target = outputDir + u'/' + relative;
    if (!QDir().mkpath(QFileInfo(target).absolutePath())) {
        setError(error, QCoreApplication::translate("Crowbar", "Could not create the folder for \"%1\".").arg(relative));
        return false;
    }
    QFile out(target);
    if (!out.open(QIODevice::WriteOnly)) {
        setError(error, QStringLiteral("%1: %2").arg(relative, out.errorString()));
        return false;
    }
    out.write(entry.preload);

    if (entry.length > 0) {
        QFile in(entry.archive);
        if (!in.open(QIODevice::ReadOnly) || !in.seek(qint64(entry.offset))) {
            setError(error, QCoreApplication::translate("Crowbar", "%1: cannot read \"%2\".").arg(relative, QFileInfo(entry.archive).fileName()));
            out.remove();
            return false;
        }
        quint64 remaining = entry.length;
        while (remaining > 0) {
            const QByteArray chunk = in.read(qint64(qMin<quint64>(remaining, 1 << 20)));
            if (chunk.isEmpty() || out.write(chunk) != chunk.size()) {
                setError(error, QCoreApplication::translate("Crowbar", "%1: data is truncated or the disk is full.").arg(relative));
                out.remove();
                return false;
            }
            remaining -= quint64(chunk.size());
        }
    }
    return true;
}

ExtractJob::ExtractJob(const Package &package, const QList<int> &entryIndexes, const QString &outputDir, QObject *parent)
    : QThread(parent)
    , m_package(package)
    , m_indexes(entryIndexes)
    , m_outputDir(outputDir)
{
}

void ExtractJob::run()
{
    int extracted = 0;
    int failed = 0;
    const int total = int(m_indexes.size());
    for (int i = 0; i < total; ++i) {
        if (isInterruptionRequested()) {
            emit done(extracted, failed, true);
            return;
        }
        QString error;
        if (m_package.extract(m_package.entries().at(m_indexes[i]), m_outputDir, &error)) {
            ++extracted;
        } else {
            ++failed;
            emit message(QCoreApplication::translate("Crowbar", "WARNING: ") + error);
        }
        if (i % 64 == 0 || i + 1 == total)
            emit progress(i + 1, total);
    }
    emit done(extracted, failed, false);
}

} // namespace package
