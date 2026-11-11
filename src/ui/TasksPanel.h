#pragma once

#include <QWidget>

class LogView;
class QLabel;
class QListWidget;
class QListWidgetItem;
class QPushButton;
class QToolButton;
class Task;
class TaskManager;

// Bottom panel: the list of tasks and the log of the selected one.
class TasksPanel : public QWidget
{
	Q_OBJECT

public:
	TasksPanel(TaskManager *tasks, QWidget *parent = nullptr);

	bool isExpanded() const { return m_expanded; }
	void setExpanded(bool expanded);

signals:
	void expandedChanged(bool expanded);

private:
	void addTask(Task *task);
	void removeTask(Task *task);
	void updateItem(Task *task);
	void showSelected();
	void updateSummary();
	QListWidgetItem *itemFor(Task *task) const;
	Task *selectedTask() const;

	TaskManager *m_tasks;
	bool m_expanded = true;
	QToolButton *m_toggle;
	QLabel *m_summary;
	QPushButton *m_cancel;
	QPushButton *m_clear;
	QWidget *m_body;
	QListWidget *m_list;
	LogView *m_log;
};
