#include "ui/About.h"

#include <QCoreApplication>
#include <QMessageBox>

QString versionString()
{
	const QString commit = QStringLiteral(CROWBAR_COMMIT);
	return commit.isEmpty() ? QStringLiteral(CROWBAR_VERSION) : QStringLiteral(CROWBAR_VERSION " (g%1)").arg(commit);
}

void showAboutDialog(QWidget *parent)
{
	const QString text = QCoreApplication::translate(
							 "About",
							 "<h3>Crowbar Qt %1</h3>"
							 "<p>A native Linux tool for modding Source engine games: compile, view, pack, unpack and publish, "
							 "for games installed natively or through Proton.</p>"
							 "<p>Built with Qt %2, running on Qt %3.</p>"
							 "<p>Free software under the GNU General Public License v3 or later.<br>"
							 "<a href=\"https://github.com/JonnyBro/crowbar-qt\">github.com/JonnyBro/crowbar-qt</a></p>"
							 "<p>Written from scratch after "
							 "<a href=\"https://steamcommunity.com/groups/CrowbarTool\">Crowbar</a> by ZeqMacaw.</p>")
							 .arg(versionString(), QStringLiteral(QT_VERSION_STR), QString::fromLatin1(qVersion()));
	QMessageBox::about(parent, QCoreApplication::translate("About", "About Crowbar Qt"), text);
}
