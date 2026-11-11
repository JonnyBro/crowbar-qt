#include "core/Packer.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QRegularExpression>
#include <QSaveFile>

#include <algorithm>
#include <functional>
#include <optional>

namespace packer
{

	namespace
	{

		bool moveFile(const QString &source, const QString &target, QStringList &log)
		{
			if (QDir::cleanPath(source) == QDir::cleanPath(target))
				return true;
			QDir().mkpath(QFileInfo(target).absolutePath());
			QFile::remove(target);
			if (!QFile::rename(source, target))
			{
				log << QCoreApplication::translate("Crowbar", "WARNING: Could not move \"%1\" to \"%2\".").arg(source, target);
				return false;
			}
			log << QCoreApplication::translate("Crowbar", "Moved \"%1\" to \"%2\".").arg(QFileInfo(source).fileName(), QFileInfo(target).absolutePath());
			return true;
		}


		// Minimal scan of a JSON object's top level: where each member's key and
		// value sit in the text, so single values can be replaced in place.
		struct Member
		{
			QString key;
			qsizetype keyStart = 0;
			qsizetype keyEnd = 0;
			qsizetype valueStart = 0;
			qsizetype valueEnd = 0;
		};

		struct ObjectLayout
		{
			qsizetype open = 0;
			qsizetype close = 0;
			QList<Member> members;
		};

		qsizetype skipSpace(const QString &text, qsizetype pos)
		{
			while (pos < text.size() && (text[pos].isSpace() || text[pos] == QChar(0xFEFF)))
				++pos;
			return pos;
		}

		// pos is at the opening quote; returns the index just past the closing one.
		qsizetype skipString(const QString &text, qsizetype pos)
		{
			for (++pos; pos < text.size(); ++pos)
			{
				if (text[pos] == u'\\')
					++pos;
				else if (text[pos] == u'"')
					return pos + 1;
			}
			return -1;
		}

		qsizetype skipValue(const QString &text, qsizetype pos)
		{
			if (pos >= text.size())
				return -1;
			if (text[pos] == u'"')
				return skipString(text, pos);
			if (text[pos] == u'{' || text[pos] == u'[')
			{
				int depth = 0;
				while (pos < text.size())
				{
					const QChar c = text[pos];
					if (c == u'"')
					{
						pos = skipString(text, pos);
						if (pos < 0)
							return -1;
						continue;
					}
					if (c == u'{' || c == u'[')
						++depth;
					else if ((c == u'}' || c == u']') && --depth == 0)
						return pos + 1;
					++pos;
				}
				return -1;
			}
			while (pos < text.size() && !text[pos].isSpace() && text[pos] != u',' && text[pos] != u'}' && text[pos] != u']')
				++pos;
			return pos;
		}

		std::optional<ObjectLayout> scanObject(const QString &text)
		{
			ObjectLayout layout;
			qsizetype pos = skipSpace(text, 0);
			if (pos >= text.size() || text[pos] != u'{')
				return std::nullopt;
			layout.open = pos++;
			for (;;)
			{
				pos = skipSpace(text, pos);
				if (pos >= text.size())
					return std::nullopt;
				if (text[pos] == u'}')
				{
					layout.close = pos;
					return layout;
				}
				if (text[pos] != u'"')
					return std::nullopt;
				Member member;
				member.keyStart = pos;
				member.keyEnd = skipString(text, pos);
				if (member.keyEnd < 0)
					return std::nullopt;
				member.key = text.mid(member.keyStart + 1, member.keyEnd - member.keyStart - 2);
				pos = skipSpace(text, member.keyEnd);
				if (pos >= text.size() || text[pos] != u':')
					return std::nullopt;
				member.valueStart = skipSpace(text, pos + 1);
				member.valueEnd = skipValue(text, member.valueStart);
				if (member.valueEnd < 0)
					return std::nullopt;
				layout.members << member;
				pos = skipSpace(text, member.valueEnd);
				if (pos < text.size() && text[pos] == u',')
					++pos;
			}
		}

		QString lineIndent(const QString &text, qsizetype pos)
		{
			qsizetype start = pos;
			while (start > 0 && text[start - 1] != u'\n')
				--start;
			const QString prefix = text.mid(start, pos - start);
			return prefix.trimmed().isEmpty() ? prefix : QString();
		}

		QString jsonString(const QString &value)
		{
			const QByteArray compact = QJsonDocument(QJsonArray{value}).toJson(QJsonDocument::Compact);
			return QString::fromUtf8(compact.mid(1, compact.size() - 2));
		}

		// Writes the tags the way the file already wrote its array.
		QString formatTags(const QStringList &tags, const QString &original, const QString &keyIndent, const QString &newline)
		{
			QStringList items;
			for (const QString &tag : tags)
				items << jsonString(tag);
			if (items.isEmpty())
				return QStringLiteral("[]");
			if (original.contains(u'\n'))
			{
				QString itemIndent = keyIndent + (keyIndent.startsWith(u' ') ? QStringLiteral("    ") : QStringLiteral("\t"));
				const qsizetype first = original.indexOf(u'"');
				if (first > 0)
				{
					const QString before = original.left(first);
					const qsizetype newlineAt = before.lastIndexOf(u'\n');
					if (newlineAt >= 0 && before.mid(newlineAt + 1).trimmed().isEmpty())
						itemIndent = before.mid(newlineAt + 1);
				}
				return u'[' + newline + itemIndent + items.join(u',' + newline + itemIndent) + newline + keyIndent + u']';
			}
			const QString pad = original.startsWith(QStringLiteral("[ ")) ? QStringLiteral(" ") : QString();
			const QString separator = original.contains(QStringLiteral(", ")) || !original.contains(u',') ? QStringLiteral(", ") : QStringLiteral(",");
			return u'[' + pad + items.join(separator) + pad + u']';
		}

		QString newAddonJson(const AddonInfo &info)
		{
			const QString tags = formatTags(info.tags, {}, QStringLiteral("\t"), QStringLiteral("\n"));
			return QStringLiteral("{\n\t\"title\": %1,\n\t\"type\": %2,\n\t\"tags\": %3,\n\t\"ignore\": [\"*.psd\", \"*.vcproj\", \"*.svn*\"]\n}\n")
				.arg(jsonString(info.title), jsonString(info.type), tags);
		}

		// Replaces title, type and tags inside the existing text; everything else
		// (key order, indentation, line endings, other keys) stays byte for byte.
		std::optional<QString> updateAddonJson(const QString &text, const AddonInfo &info)
		{
			const auto layout = scanObject(text);
			if (!layout)
				return std::nullopt;
			const QString newline = text.contains(QStringLiteral("\r\n")) ? QStringLiteral("\r\n") : QStringLiteral("\n");
			const QString indent = layout->members.isEmpty() ? QStringLiteral("\t") : lineIndent(text, layout->members.first().keyStart);
			const QString separator = layout->members.isEmpty()
										  ? QStringLiteral(": ")
										  : text.mid(layout->members.first().keyEnd, layout->members.first().valueStart - layout->members.first().keyEnd);

			struct Edit
			{
				qsizetype start;
				qsizetype end;
				QString text;
			};
			QList<Edit> edits;
			const QList<std::pair<QString, std::function<QString(const QString &, const QString &)>>> fields = {
				{"title", [&](const QString &, const QString &)
				 { return jsonString(info.title); }},
				{"type", [&](const QString &, const QString &)
				 { return jsonString(info.type); }},
				{"tags", [&](const QString &original, const QString &keyIndent)
				 { return formatTags(info.tags, original, keyIndent, newline); }},
			};
			QStringList inserts;
			for (const auto &[key, format] : fields)
			{
				bool found = false;
				for (const Member &member : layout->members)
				{
					if (member.key != key)
						continue;
					found = true;
					const QString original = text.mid(member.valueStart, member.valueEnd - member.valueStart);
					edits.append({member.valueStart, member.valueEnd, format(original, lineIndent(text, member.keyStart))});
				}
				if (!found)
					inserts << jsonString(key) + separator + format({}, indent);
			}
			if (!inserts.isEmpty())
			{
				if (layout->members.isEmpty())
				{
					edits.append({layout->open + 1, layout->open + 1, newline + indent + inserts.join(u',' + newline + indent) + newline});
				}
				else
				{
					const qsizetype end = layout->members.last().valueEnd;
					edits.append({end, end, u',' + newline + indent + inserts.join(u',' + newline + indent)});
				}
			}

			std::sort(edits.begin(), edits.end(), [](const Edit &a, const Edit &b)
					  { return a.start > b.start; });
			QString result = text;
			for (const Edit &edit : edits)
				result.replace(edit.start, edit.end - edit.start, edit.text);
			return result;
		}

	} // namespace

	QString toolName(Format format)
	{
		return format == Format::Vpk ? "vpk" : "gmad";
	}

	QString extension(Format format)
	{
		return format == Format::Vpk ? "vpk" : "gma";
	}

	launcher::Command packCommand(const steam::App &app, Format format, const QString &folder,
								  const QString &outputFile, const Options &options)
	{
		launcher::Command command;
		const QString executable = launcher::findTool(app, toolName(format));
		if (executable.isEmpty())
		{
			command.error = QCoreApplication::translate("Crowbar", "%1 has no %2 in its bin folder.").arg(app.name, toolName(format));
			return command;
		}
		const QFileInfo input(QDir::cleanPath(folder));
		if (!input.isDir())
		{
			command.error = QCoreApplication::translate("Crowbar", "\"%1\" is not a folder.").arg(folder);
			return command;
		}
		if (format == Format::Gma && !hasAddonJson(input.absoluteFilePath()))
		{
			command.error = QCoreApplication::translate("Crowbar", "gmad needs an addon.json file in \"%1\".").arg(folder);
			return command;
		}

		const bool windows = launcher::isWindowsExecutable(executable);
		const auto path = [&](const QString &hostPath)
		{
			return windows ? launcher::toWindowsPath(hostPath) : QDir::cleanPath(hostPath);
		};

		QStringList arguments;
		if (format == Format::Vpk)
		{
			if (options.multiChunk)
				arguments << "-M";
			arguments += options.extra;
			arguments << path(input.absoluteFilePath());
		}
		else
		{
			QDir().mkpath(QFileInfo(outputFile).absolutePath());
			arguments << "create" << "-folder" << path(input.absoluteFilePath()) << "-out" << path(outputFile);
			if (options.warnInvalid)
				arguments << "-warninvalid";
			arguments += options.extra;
		}

		command = launcher::build(app, executable, arguments, launcher::Mode::Console);
		command.workingDirectory = input.absolutePath();
		return command;
	}

	QStringList finishPack(Format format, const QString &folder, const QString &outputFile,
						   const Options &options, const QDateTime &started, QStringList &log)
	{
		if (format == Format::Gma)
		{
			if (QFile::exists(outputFile))
				return {outputFile};
			log << QCoreApplication::translate("Crowbar", "WARNING: The packer did not create \"%1\".").arg(outputFile);
			return {};
		}

		const QFileInfo input(QDir::cleanPath(folder));
		const QFileInfo output(outputFile);
		const QString sourceBase = input.absolutePath() + u'/' + input.fileName();
		const QString targetBase = output.absolutePath() + u'/' + output.completeBaseName();

		if (!options.multiChunk)
		{
			const QString packed = sourceBase + ".vpk";
			if (!QFile::exists(packed))
			{
				log << QCoreApplication::translate("Crowbar", "WARNING: The packer did not create \"%1\".").arg(packed);
				return {};
			}
			return moveFile(packed, outputFile, log) ? QStringList{outputFile} : QStringList{};
		}

		static const QRegularExpression chunkSuffix("_(dir|\\d{3})\\.vpk$", QRegularExpression::CaseInsensitiveOption);
		QStringList result;
		const QDir parent(input.absolutePath());
		for (const QFileInfo &file : parent.entryInfoList({input.fileName() + "_*.vpk"}, QDir::Files, QDir::Name))
		{
			const auto match = chunkSuffix.match(file.fileName());
			if (!match.hasMatch() || file.fileName().left(match.capturedStart()) != input.fileName())
				continue;
			if (file.lastModified() < started.addSecs(-2))
				continue;
			const QString target = targetBase + match.captured(0).toLower();
			if (moveFile(file.absoluteFilePath(), target, log))
				result << target;
		}
		if (result.isEmpty())
			log << QCoreApplication::translate("Crowbar", "WARNING: The packer did not create \"%1_dir.vpk\".").arg(sourceBase);
		return result;
	}

	QStringList addonTypes()
	{
		return {"gamemode", "map", "weapon", "vehicle", "npc", "entity", "tool", "effects", "model", "servercontent"};
	}

	QStringList addonTags()
	{
		return {"fun", "roleplay", "scenic", "movie", "realism", "cartoon", "water", "comic", "build"};
	}

	bool hasAddonJson(const QString &folder)
	{
		return QFile::exists(folder + "/addon.json");
	}

	std::optional<AddonInfo> readAddonJson(const QString &folder, QString *error)
	{
		QFile file(folder + "/addon.json");
		if (!file.open(QIODevice::ReadOnly))
		{
			if (error)
				*error = file.errorString();
			return std::nullopt;
		}
		QJsonParseError parseError;
		const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &parseError);
		if (!document.isObject())
		{
			if (error)
				*error = QCoreApplication::translate("Crowbar", "addon.json is not valid JSON: %1").arg(parseError.errorString());
			return std::nullopt;
		}
		const QJsonObject root = document.object();
		AddonInfo info;
		info.title = root["title"].toString();
		info.type = root["type"].toString().toLower();
		for (const QJsonValue &tag : root["tags"].toArray())
			info.tags << tag.toString().toLower();
		return info;
	}

	bool saveAddonJson(const QString &folder, const AddonInfo &info, QString *error)
	{
		const QString path = folder + "/addon.json";
		QString text;
		if (QFile::exists(path))
		{
			QFile in(path);
			if (!in.open(QIODevice::ReadOnly))
			{
				if (error)
					*error = in.errorString();
				return false;
			}
			const QByteArray original = in.readAll();
			QJsonParseError parseError;
			const auto updated = QJsonDocument::fromJson(original, &parseError).isObject() ? updateAddonJson(QString::fromUtf8(original), info) : std::nullopt;
			if (!updated)
			{
				if (error)
					*error = QCoreApplication::translate("Crowbar", "addon.json is not valid JSON, so it was left as is: %1").arg(parseError.errorString());
				return false;
			}
			text = *updated;
		}
		else
		{
			text = newAddonJson(info);
		}

		// Never write a file that would no longer parse or that lost the new values.
		const QJsonObject check = QJsonDocument::fromJson(text.toUtf8()).object();
		QStringList checkTags;
		for (const QJsonValue &tag : check["tags"].toArray())
			checkTags << tag.toString();
		if (check["title"].toString() != info.title || check["type"].toString() != info.type || checkTags != info.tags)
		{
			if (error)
				*error = QCoreApplication::translate("Crowbar", "addon.json could not be updated safely, so it was left as is.");
			return false;
		}

		QSaveFile out(path);
		if (!out.open(QIODevice::WriteOnly) || out.write(text.toUtf8()) < 0 || !out.commit())
		{
			if (error)
				*error = out.errorString();
			return false;
		}
		return true;
	}

	bool writeAddonJson(const QString &folder, const QString &title, const QString &type,
						const QStringList &tags, QString *error)
	{
		QJsonObject root;
		root["title"] = title;
		root["type"] = type;
		root["tags"] = QJsonArray::fromStringList(tags);
		root["ignore"] = QJsonArray::fromStringList({"*.psd", "*.vcproj", "*.svn*"});

		QFile file(folder + "/addon.json");
		// NewOnly: an existing addon.json is the user's and is never replaced.
		if (!file.open(QIODevice::WriteOnly | QIODevice::NewOnly))
		{
			if (error)
				*error = file.errorString();
			return false;
		}
		file.write(QJsonDocument(root).toJson(QJsonDocument::Indented));
		return true;
	}

} // namespace packer
