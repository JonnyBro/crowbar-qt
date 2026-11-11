#pragma once

#include "ui/Page.h"

class QLineEdit;
class QPushButton;

class ViewPage : public Page {
    Q_OBJECT

public:
    ViewPage(GameList *games, TaskManager *tasks, QWidget *parent = nullptr);
    ~ViewPage() override;

    void setModel(const QString &mdlFile);

private:
    void launch(bool withModel);
    void updateButtons();

    QLineEdit *m_mdlFile;
    QPushButton *m_view;
    QPushButton *m_openViewer;
};
