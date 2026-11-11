#include "ui/PathField.h"

#include <QAction>
#include <QCompleter>
#include <QDragEnterEvent>
#include <QDropEvent>
#include <QFileInfo>
#include <QFileSystemModel>
#include <QIcon>
#include <QLineEdit>
#include <QMenu>
#include <QMimeData>
#include <QSettings>
#include <QStyle>

namespace
{

	constexpr int HistorySize = 15;

	QString settingsKey(const QString &historyKey)
	{
		return "history/" + historyKey;
	}

	class DropFilter : public QObject
	{
	public:
		DropFilter(QLineEdit *edit, PathKind kind)
			: QObject(edit), m_edit(edit), m_kind(kind)
		{
		}

	protected:
		bool eventFilter(QObject *, QEvent *event) override
		{
			if (event->type() == QEvent::DragEnter || event->type() == QEvent::DragMove)
			{
				auto *drag = static_cast<QDropEvent *>(event);
				if (droppedPath(drag->mimeData()).isEmpty())
					return false;
				drag->acceptProposedAction();
				return true;
			}
			if (event->type() == QEvent::Drop)
			{
				auto *drop = static_cast<QDropEvent *>(event);
				const QString path = droppedPath(drop->mimeData());
				if (path.isEmpty())
					return false;
				m_edit->setText(path);
				emit m_edit->editingFinished();
				drop->acceptProposedAction();
				return true;
			}
			return false;
		}

	private:
		QString droppedPath(const QMimeData *mime) const
		{
			if (!mime->hasUrls() || mime->urls().isEmpty() || !mime->urls().first().isLocalFile())
				return {};
			const QFileInfo info(mime->urls().first().toLocalFile());
			const bool matches = m_kind == PathKind::Any || (m_kind == PathKind::Folder ? info.isDir() : info.isFile());
			return matches ? info.absoluteFilePath() : QString();
		}

		QLineEdit *m_edit;
		PathKind m_kind;
	};

} // namespace

void setUpPathField(QLineEdit *edit, const QString &historyKey, PathKind kind)
{
	auto *model = new QFileSystemModel(edit);
	model->setRootPath("/");
	model->setFilter(kind == PathKind::Folder ? QDir::AllDirs | QDir::NoDotAndDotDot
											  : QDir::AllEntries | QDir::NoDotAndDotDot);
	auto *completer = new QCompleter(model, edit);
	completer->setCaseSensitivity(Qt::CaseInsensitive);
	edit->setCompleter(completer);

	edit->installEventFilter(new DropFilter(edit, kind));

	const QIcon icon = QIcon::fromTheme("document-open-recent", edit->style()->standardIcon(QStyle::SP_ArrowDown));
	QAction *recent = edit->addAction(icon, QLineEdit::TrailingPosition);
	recent->setToolTip(QObject::tr("Recent paths"));
	QObject::connect(recent, &QAction::triggered, edit, [edit, historyKey]
					 {
		const QStringList history = QSettings().value(settingsKey(historyKey)).toStringList();
		QMenu menu(edit);
		for (const QString &path : history)
		{
			QAction *item = menu.addAction(path);
			item->setEnabled(QFileInfo::exists(path));
			QObject::connect(item, &QAction::triggered, edit, [edit, path]
							 {
				edit->setText(path);
				emit edit->editingFinished(); });
		}
		if (history.isEmpty())
			menu.addAction(QObject::tr("No recent paths"))->setEnabled(false);
		menu.exec(edit->mapToGlobal(QPoint(edit->width() - menu.sizeHint().width(), edit->height()))); });
}

void rememberPath(const QString &historyKey, const QString &path)
{
	const QString cleaned = path.trimmed();
	if (cleaned.isEmpty())
		return;
	QSettings settings;
	QStringList history = settings.value(settingsKey(historyKey)).toStringList();
	history.removeAll(cleaned);
	history.prepend(cleaned);
	settings.setValue(settingsKey(historyKey), history.mid(0, HistorySize));
}
