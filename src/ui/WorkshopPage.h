#pragma once

#include "core/Publisher.h"
#include "ui/Page.h"


class QLabel;
class QLineEdit;
class QPushButton;
class QTreeWidget;
class QTreeWidgetItem;
class Task;

namespace workshop
{
    class Client;
}

// Your workshop items, and creating or updating one with gmpublish.
class WorkshopPage : public Page {
    Q_OBJECT

public:
    WorkshopPage(GameList *games, TaskManager *tasks, QWidget *parent = nullptr);
    ~WorkshopPage() override;

private:
    void refreshList();
    void showItems(const QList<publisher::WorkshopItem> &items);
    void browseContent();
    void publish();
    void upload(const publisher::Request &request);
    void updateWidgets();
    void saveSettings() const;

    workshop::Client *m_client;
    bool m_busy = false;

    QTreeWidget *m_items;
    QPushButton *m_refresh;
    QLineEdit *m_itemId;
    QLabel *m_itemHint;
    QLineEdit *m_content;
    QLineEdit *m_icon;
    QLineEdit *m_changes;
    QPushButton *m_publish;
};
