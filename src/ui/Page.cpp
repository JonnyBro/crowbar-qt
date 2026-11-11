#include "ui/Page.h"

#include <QLabel>
#include <QVBoxLayout>

Page::Page(const QString &title, GameList *games, TaskManager *tasks, QWidget *parent)
	: QWidget(parent), m_games(games), m_tasks(tasks), m_notice(new QLabel(this)), m_content(new QVBoxLayout)
{
	auto *heading = new QLabel(title, this);
	QFont font = heading->font();
	font.setPointSizeF(font.pointSizeF() * 1.35);
	heading->setFont(font);

	m_notice->setWordWrap(true);
	m_notice->setVisible(false);
	m_notice->setForegroundRole(QPalette::PlaceholderText);

	auto *layout = new QVBoxLayout(this);
	layout->addWidget(heading);
	layout->addWidget(m_notice);
	layout->addLayout(m_content, 1);
}

void Page::setNotice(const QString &text)
{
	m_notice->setText(text);
	m_notice->setVisible(!text.isEmpty());
}
