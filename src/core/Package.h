#pragma once

#include <QByteArray>
#include <QList>
#include <QString>
#include <QThread>

#include <memory>
#include <optional>

class QTemporaryFile;

namespace package {

struct Entry {
    QString path;
    quint64 size = 0;
    quint32 crc = 0;

    // Where the data lives: `preload` first, then `length` bytes at `offset` of `archive`.
    QString archive;
    quint64 offset = 0;
    quint64 length = 0;
    QByteArray preload;
};

class Package {
public:
    // Reads the file table of a VPK (v1/v2) or GMA, including the LZMA-compressed
    // GMA files (*.bin) that old workshop items are stored as. For a numbered VPK part
    // such as pak01_003.vpk, the matching pak01_dir.vpk is opened instead.
    static std::optional<Package> open(const QString &file, QString *error = nullptr);

    QString file() const { return m_file; }
    QString format() const { return m_format; }
    QString title() const { return m_title; }
    const QList<Entry> &entries() const { return m_entries; }

    bool extract(const Entry &entry, const QString &outputDir, QString *error = nullptr) const;
    // The entry's data in memory, for previews; fails for entries over `maxSize` bytes.
    std::optional<QByteArray> read(const Entry &entry, quint64 maxSize, QString *error = nullptr) const;

private:
    QString m_file;
    QString m_format;
    QString m_title;
    QList<Entry> m_entries;
    // Decompressed copy of an LZMA-compressed GMA; entries point into it.
    std::shared_ptr<QTemporaryFile> m_decompressed;

    friend std::optional<Package> readVpk(const QString &file, QString *error);
    friend std::optional<Package> readGma(const QString &file, QString *error);
};

// Legacy LZMA-compressed workshop files (*_legacy.bin).
bool isLzmaFile(const QString &file);
bool decompressLzmaFile(const QString &file, const QString &outputFile, QString *error = nullptr);

class ExtractJob : public QThread {
    Q_OBJECT

public:
    ExtractJob(const Package &package, const QList<int> &entryIndexes, const QString &outputDir,
               QObject *parent = nullptr);

signals:
    void progress(int done, int total);
    void message(const QString &text);
    void done(int extracted, int failed, bool cancelled);

protected:
    void run() override;

private:
    Package m_package;
    QList<int> m_indexes;
    QString m_outputDir;
};

} // namespace package
