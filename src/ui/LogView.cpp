#include "ui/LogView.h"

#include "core/Launcher.h"

#include <QDesktopServices>
#include <QFileInfo>
#include <QFontDatabase>
#include <QMouseEvent>
#include <QProcess>
#include <QRegularExpression>
#include <QScrollBar>
#include <QTextBlock>
#include <QTextCursor>
#include <QSettings>
#include <QUrl>
#include <QUrlQuery>

namespace
{

	enum class Severity
	{
		Normal,
		Warning,
		Error
	};

	Severity severityOf(QStringView line)
	{
		const QStringView text = line.trimmed();
		for (QStringView prefix : {u"ERROR", u"ОШИБКА", u"FATAL"})
		{
			if (text.startsWith(prefix, Qt::CaseInsensitive))
				return Severity::Error;
		}
		for (QStringView prefix : {u"WARNING", u"ВНИМАНИЕ"})
		{
			if (text.startsWith(prefix, Qt::CaseInsensitive))
				return Severity::Warning;
		}
		return Severity::Normal;
	}

	bool isTextFile(const QString &path)
	{
		static const QStringList text{"qc", "qci", "smd", "vta", "txt", "vmt", "lua", "json", "cfg", "fgd", "res"};
		return text.contains(QFileInfo(path).suffix().toLower());
	}

	struct Segment
	{
		QString text;
		QString href;
	};

	QList<Segment> linkify(const QString &line)
	{
		// A quoted path may contain spaces; a bare one ends at whitespace or a quote.
		// "(12)" right after a path is a line number, as studiomdl prints it.
		static const QRegularExpression pattern(R"re("(/[^"]+)"|(/[^\s"'<>|]+?)(?=[\s"':,]|\(\d+\)|$))re");
		static const QRegularExpression lineNumber(R"(^\((\d+)\))");

		QList<Segment> segments;
		qsizetype last = 0;
		auto it = pattern.globalMatch(line);
		while (it.hasNext())
		{
			const auto match = it.next();
			const bool quoted = match.capturedLength(1) > 0;
			QString path = quoted ? match.captured(1) : match.captured(2);
			while (path.endsWith(u'.') || path.endsWith(u')'))
				path.chop(1);
			const QString real = launcher::findPathIgnoringCase({}, path);
			if (real.isEmpty() || real == "/")
				continue;

			const qsizetype start = match.capturedStart(quoted ? 1 : 2);
			if (start < last)
				continue;
			qsizetype end = start + path.size();
			int lineNo = 0;
			const auto lineMatch = lineNumber.matchView(QStringView(line).mid(end));
			if (lineMatch.hasMatch())
			{
				lineNo = lineMatch.captured(1).toInt();
				end += lineMatch.capturedLength(0);
			}

			QUrl url = QUrl::fromLocalFile(real);
			if (lineNo > 0)
				url.setQuery(QStringLiteral("line=%1").arg(lineNo));
			segments.append({line.mid(last, start - last), {}});
			segments.append({line.mid(start, end - start), url.toString(QUrl::FullyEncoded)});
			last = end;
		}
		segments.append({line.mid(last), {}});
		return segments;
	}

} // namespace

void openInEditor(const QString &file, int line)
{
	// e.g. "code -g %f:%l", "kate --line %l %f", "subl %f:%l"; empty uses the desktop default.
	const QString command = QSettings().value("editor/command").toString().trimmed();
	if (!command.isEmpty())
	{
		QStringList arguments = QProcess::splitCommand(command);
		const QString program = arguments.takeFirst();
		for (QString &argument : arguments)
		{
			argument.replace("%f", file);
			argument.replace("%l", QString::number(qMax(1, line)));
		}
		if (QProcess::startDetached(program, arguments))
			return;
	}
	QDesktopServices::openUrl(QUrl::fromLocalFile(file));
}

LogView::LogView(QWidget *parent)
	: QPlainTextEdit(parent)
{
	setReadOnly(true);
	setLineWrapMode(QPlainTextEdit::NoWrap);
	setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
	setMouseTracking(true);
	viewport()->setMouseTracking(true);
}

void LogView::appendLine(const QString &line)
{
	QScrollBar *scroll = verticalScrollBar();
	const bool atBottom = scroll->value() == scroll->maximum();

	QTextCharFormat base;
	switch (severityOf(line))
	{
	case Severity::Error:
		base.setForeground(QColor(0xe5, 0x53, 0x4b));
		break;
	case Severity::Warning:
		base.setForeground(QColor(0xc6, 0x90, 0x26));
		break;
	case Severity::Normal:
		break;
	}
	QTextCharFormat link = base;
	link.setAnchor(true);
	link.setFontUnderline(true);
	if (!base.hasProperty(QTextFormat::ForegroundBrush))
		link.setForeground(palette().link());

	QTextCursor cursor(document());
	cursor.movePosition(QTextCursor::End);
	if (!document()->isEmpty())
		cursor.insertBlock(QTextBlockFormat(), base);
	for (const Segment &segment : linkify(line))
	{
		if (segment.text.isEmpty())
			continue;
		QTextCharFormat format = segment.href.isEmpty() ? base : link;
		format.setAnchorHref(segment.href);
		cursor.insertText(segment.text, format);
	}

	if (atBottom)
		scroll->setValue(scroll->maximum());
}

void LogView::mouseMoveEvent(QMouseEvent *event)
{
	viewport()->setCursor(anchorAt(event->position().toPoint()).isEmpty() ? Qt::IBeamCursor : Qt::PointingHandCursor);
	QPlainTextEdit::mouseMoveEvent(event);
}

void LogView::mouseReleaseEvent(QMouseEvent *event)
{
	const QString anchor = anchorAt(event->position().toPoint());
	// Only a plain click follows a link, so text can still be selected across it.
	if (event->button() == Qt::LeftButton && !anchor.isEmpty() && !textCursor().hasSelection())
	{
		const QUrl url(anchor);
		const QString file = url.toLocalFile();
		const int line = QUrlQuery(url).queryItemValue("line").toInt();
		if (QFileInfo(file).isDir())
			QDesktopServices::openUrl(url.adjusted(QUrl::RemoveQuery));
		else if (isTextFile(file))
			openInEditor(file, line);
		else
			QDesktopServices::openUrl(QUrl::fromLocalFile(QFileInfo(file).absolutePath()));
	}
	QPlainTextEdit::mouseReleaseEvent(event);
}
