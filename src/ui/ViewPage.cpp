#include "ui/ViewPage.h"

#include "core/Hlmv.h"
#include "ui/GameList.h"
#include "ui/PathField.h"
#include "ui/Tasks.h"

#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QPushButton>
#include <QSettings>
#include <QVBoxLayout>

ViewPage::ViewPage(GameList *games, TaskManager *tasks, QWidget *parent)
	: Page(tr("View"), games, tasks, parent), m_mdlFile(new QLineEdit(this)), m_view(new QPushButton(tr("View"), this)),
	  m_openViewer(new QPushButton(tr("Open Viewer"), this))
{
	setUpPathField(m_mdlFile, "mdl", PathKind::File);
	m_mdlFile->setText(QSettings().value("view/mdl").toString());
	m_mdlFile->setPlaceholderText(tr("Path to a .mdl file"));

	auto *browse = new QPushButton(tr("Browse…"), this);
	auto *mdlRow = new QHBoxLayout;
	mdlRow->addWidget(m_mdlFile, 1);
	mdlRow->addWidget(browse);

	auto *form = new QFormLayout;
	form->addRow(tr("MDL file:"), mdlRow);

	auto *buttons = new QHBoxLayout;
	buttons->addStretch(1);
	buttons->addWidget(m_openViewer);
	buttons->addWidget(m_view);
	m_view->setDefault(true);

	content()->addLayout(form);
	content()->addLayout(buttons);
	content()->addStretch(1);

	connect(browse, &QPushButton::clicked, this, [this]
			{
		const QString file = QFileDialog::getOpenFileName(this, tr("Select MDL file"), QFileInfo(m_mdlFile->text()).absolutePath(),
														  tr("MDL files (*.mdl);;All files (*)"));
		if (!file.isEmpty())
			m_mdlFile->setText(file); });
	connect(m_view, &QPushButton::clicked, this, [this]
			{ launch(true); });
	connect(m_openViewer, &QPushButton::clicked, this, [this]
			{ launch(false); });
	connect(m_games, &GameList::currentChanged, this, &ViewPage::updateButtons);
	updateButtons();
}

ViewPage::~ViewPage()
{
	QSettings().setValue("view/mdl", m_mdlFile->text());
}

void ViewPage::setModel(const QString &mdlFile)
{
	m_mdlFile->setText(mdlFile);
}

void ViewPage::launch(bool withModel)
{
	const auto target = m_games->current();
	if (!target)
		return;
	const QString mdlFile = withModel ? m_mdlFile->text().trimmed() : QString();
	if (withModel)
	{
		rememberPath("mdl", mdlFile);
		QSettings().setValue("view/mdl", mdlFile);
	}

	Task *task = m_tasks->start(withModel ? tr("HLMV %1").arg(QFileInfo(mdlFile).fileName()) : tr("HLMV"));
	if (withModel && mdlFile.isEmpty())
	{
		task->fail(tr("Select an MDL file."));
		return;
	}
	// Closing the viewer is the normal way for this task to end.
	task->run(hlmv::viewCommand(target->app, target->gameDir, mdlFile), [](int, bool)
			  { return true; });
}

void ViewPage::updateButtons()
{
	const auto target = m_games->current();
	const bool hasTool = target && !target->tool("hlmv").isEmpty();
	setNotice(!target		 ? tr("No Source game found. Check the Games page.")
			  : !hasTool ? tr("%1 has no hlmv. Set a tools folder on the Games page.").arg(target->app.name)
						 : QString());
	m_view->setEnabled(hasTool);
	m_openViewer->setEnabled(hasTool);
}
