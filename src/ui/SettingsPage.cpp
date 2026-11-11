#include "ui/SettingsPage.h"

#include "core/Launcher.h"
#include "ui/GameList.h"

#include <QComboBox>
#include <QFontDatabase>
#include <QFormLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QSettings>
#include <QTimer>
#include <QVBoxLayout>

namespace
{

	QStringList environmentLines(const QString &text)
	{
		return text.split(u'\n');
	}

	QLabel *note(QWidget *parent)
	{
		auto *label = new QLabel(parent);
		label->setWordWrap(true);
		label->setForegroundRole(QPalette::PlaceholderText);
		return label;
	}

} // namespace

void SettingsPage::applySaved()
{
	launcher::setToolEnvironment(environmentLines(QSettings().value("tools/environment").toString()));
}

SettingsPage::SettingsPage(GameList *games, TaskManager *tasks, QWidget *parent)
	: Page(tr("Settings"), games, tasks, parent), m_language(new QComboBox(this)), m_proton(new QComboBox(this)), m_protonNote(note(this)),
	  m_environment(new QPlainTextEdit(this)), m_environmentNote(note(this)), m_environmentTimer(new QTimer(this)), m_editor(new QComboBox(this))
{
	const QSettings settings;

	m_language->addItem(tr("System language"), QString());
	m_language->addItem(QStringLiteral("English"), QStringLiteral("en"));
	m_language->addItem(QStringLiteral("Русский"), QStringLiteral("ru"));
	m_language->setCurrentIndex(qMax(0, m_language->findData(settings.value("ui/language").toString())));
	auto *languageNote = note(this);
	languageNote->setText(tr("Takes effect the next time the program starts."));

	m_protonNote->setText(tr("Runs the Windows tools (studiomdl, hlmv, Hammer, …) with this Proton instead of the game's own. "
							 "They still use the game's prefix, and a different Proton version may update it."));

	m_environment->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
	m_environment->setPlaceholderText(tr("One per line: NAME=value sets a variable, -NAME removes it.\n"
										 "For example:\n-MANGOHUD\nDXVK_HUD=0"));
	m_environment->setPlainText(settings.value("tools/environment").toString());
	m_environment->setMaximumHeight(m_environment->fontMetrics().lineSpacing() * 7);
	m_environmentTimer->setSingleShot(true);
	m_environmentTimer->setInterval(400);

	m_editor->setEditable(true);
	m_editor->addItem(tr("Desktop default (no line numbers)"), QString());
	m_editor->addItem(QStringLiteral("VS Code"), QStringLiteral("code -g %f:%l"));
	m_editor->addItem(QStringLiteral("VSCodium"), QStringLiteral("codium -g %f:%l"));
	m_editor->addItem(QStringLiteral("Kate"), QStringLiteral("kate --line %l %f"));
	m_editor->addItem(QStringLiteral("KWrite"), QStringLiteral("kwrite --line %l %f"));
	m_editor->addItem(QStringLiteral("Sublime Text"), QStringLiteral("subl %f:%l"));
	m_editor->addItem(QStringLiteral("Zed"), QStringLiteral("zed %f:%l"));
	m_editor->addItem(QStringLiteral("gedit"), QStringLiteral("gedit +%l %f"));
	const QString editor = settings.value("editor/command").toString();
	const int known = m_editor->findData(editor);
	if (known >= 0)
		m_editor->setCurrentIndex(known);
	else
		m_editor->setEditText(editor);
	m_editor->lineEdit()->setPlaceholderText(tr("Command, with %f for the file and %l for the line"));
	auto *editorNote = note(this);
	editorNote->setText(tr("Opens files from the task log: %f is the file, %l the line. Pick a preset or type a command."));

	auto *form = new QFormLayout;
	// Notes take the full width; the short lists keep their natural size.
	form->setFieldGrowthPolicy(QFormLayout::AllNonFixedFieldsGrow);
	m_language->setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
	m_proton->setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
	form->addRow(tr("Language:"), m_language);
	form->addRow(QString(), languageNote);
	form->addRow(tr("Proton for tools:"), m_proton);
	form->addRow(QString(), m_protonNote);
	form->addRow(tr("Tool environment:"), m_environment);
	form->addRow(QString(), m_environmentNote);
	form->addRow(tr("Text editor:"), m_editor);
	form->addRow(QString(), editorNote);

	content()->addLayout(form);
	content()->addStretch(1);

	connect(m_language, &QComboBox::currentIndexChanged, this, [this]
			{ QSettings().setValue("ui/language", m_language->currentData()); });
	connect(m_proton, &QComboBox::activated, this, [this]
			{
		QSettings().setValue("tools/proton", m_proton->currentData());
		m_games->refresh(); });
	connect(m_environment, &QPlainTextEdit::textChanged, m_environmentTimer, qOverload<>(&QTimer::start));
	connect(m_environmentTimer, &QTimer::timeout, this, &SettingsPage::saveEnvironment);
	// The text is the setting; a preset only fills it in.
	connect(m_editor, &QComboBox::activated, this, [this](int index)
			{
		const QString command = m_editor->itemData(index).toString();
		m_editor->setEditText(command);
		QSettings().setValue("editor/command", command); });
	connect(m_editor->lineEdit(), &QLineEdit::editingFinished, this, [this]
			{ QSettings().setValue("editor/command", m_editor->currentText().trimmed()); });
	connect(m_games, &GameList::changed, this, &SettingsPage::reloadProtons);

	reloadProtons();
	saveEnvironment();
	m_editor->setEditText(editor);
}

void SettingsPage::reloadProtons()
{
	const QString saved = QSettings().value("tools/proton").toString();
	m_proton->clear();
	m_proton->addItem(tr("The game's own (recommended)"), QString());
	for (const steam::CompatTool &tool : m_games->compatTools())
	{
		if (m_proton->findData(tool.name) < 0)
			m_proton->addItem(tool.displayName, tool.name);
	}
	const int index = m_proton->findData(saved);
	if (index < 0 && !saved.isEmpty())
		m_proton->addItem(tr("%1 (not installed)").arg(saved), saved);
	m_proton->setCurrentIndex(qMax(0, m_proton->findData(saved)));
}

void SettingsPage::saveEnvironment()
{
	const QString text = m_environment->toPlainText();
	QSettings().setValue("tools/environment", text);
	launcher::setToolEnvironment(environmentLines(text));
	QProcessEnvironment check;
	const QStringList invalid = launcher::applyEnvironmentLines(check, environmentLines(text));
	m_environmentNote->setText(invalid.isEmpty() ? tr("Applies to every tool started from now on. Proton's own variables cannot be overridden.")
												 : tr("Ignored, not NAME=value or -NAME: %1").arg(invalid.join(QStringLiteral(", "))));
}
