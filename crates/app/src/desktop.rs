//! The application menu entry, icon and file types in the user's
//! ~/.local/share, so the window opens .qc, .mdl, .vpk and .gma files from the
//! file manager.

use std::path::{Path, PathBuf};

const DESKTOP: &str = include_str!("../assets/desktop/prybar.desktop");
const ICON: &str = include_str!("../assets/desktop/prybar.svg");
const MIME: &str = include_str!("../assets/desktop/prybar-mime.xml");

struct Files {
	applications: PathBuf,
	desktop: PathBuf,
	icon: PathBuf,
	mime_root: PathBuf,
	mime: PathBuf,
}

fn files() -> Option<Files> {
	let data = directories::BaseDirs::new()?.data_dir().to_path_buf();
	let applications = data.join("applications");
	let mime_root = data.join("mime");
	Some(Files {
		desktop: applications.join("prybar.desktop"),
		applications,
		icon: data.join("icons/hicolor/scalable/apps/prybar.svg"),
		mime: mime_root.join("packages/prybar.xml"),
		mime_root,
	})
}

/// The entry runs this binary by its full path: a release archive is unpacked
/// anywhere, rarely into PATH.
fn desktop_entry(exe: &Path) -> String {
	let exe = exe.display().to_string();
	let quoted = if exe.contains([' ', '"', '\\', '$', '`']) {
		format!(
			"\"{}\"",
			exe.replace('\\', "\\\\")
				.replace('"', "\\\"")
				.replace('$', "\\$")
				.replace('`', "\\`")
		)
	} else {
		exe
	};
	DESKTOP.replace("Exec=prybar %F", &format!("Exec={quoted} %F"))
}

pub fn is_installed() -> bool {
	files().is_some_and(|f| f.desktop.exists())
}

pub fn install() -> Result<(), String> {
	let files = files().ok_or("no home folder")?;
	let exe = prybar_core::app_update::current_exe().map_err(|e| e.to_string())?;
	for (path, text) in [
		(&files.desktop, desktop_entry(&exe)),
		(&files.icon, ICON.to_owned()),
		(&files.mime, MIME.to_owned()),
	] {
		if let Some(dir) = path.parent() {
			std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
		}
		std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
	}
	refresh_caches(&files);
	Ok(())
}

pub fn uninstall() -> Result<(), String> {
	let files = files().ok_or("no home folder")?;
	for path in [&files.desktop, &files.icon, &files.mime] {
		match std::fs::remove_file(path) {
			Ok(()) => {}
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
			Err(e) => return Err(format!("{}: {e}", path.display())),
		}
	}
	refresh_caches(&files);
	Ok(())
}

/// Keeps an installed entry pointing at this binary after the archive moved,
/// and its icon and types current after an update.
pub fn refresh_if_installed() {
	let Some(files) = files() else {
		return;
	};
	let Ok(current) = std::fs::read_to_string(&files.desktop) else {
		return;
	};
	let Ok(exe) = prybar_core::app_update::current_exe() else {
		return;
	};
	let up_to_date = current == desktop_entry(&exe)
		&& std::fs::read_to_string(&files.icon).is_ok_and(|t| t == ICON)
		&& std::fs::read_to_string(&files.mime).is_ok_and(|t| t == MIME);
	if !up_to_date && let Err(e) = install() {
		tracing::warn!("could not update the menu entry: {e}");
	}
}

/// The databases are optional: without the tools, desktops pick the files up
/// on their next scan.
fn refresh_caches(files: &Files) {
	for (tool, dir) in [
		("update-mime-database", &files.mime_root),
		("update-desktop-database", &files.applications),
	] {
		let status = std::process::Command::new(tool)
			.arg(dir)
			.stdout(std::process::Stdio::null())
			.stderr(std::process::Stdio::null())
			.status();
		if let Err(e) = status {
			tracing::debug!("{tool}: {e}");
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn exec_uses_full_path() {
		let entry = desktop_entry(Path::new("/opt/prybar/prybar"));
		assert!(entry.contains("\nExec=/opt/prybar/prybar %F\n"));
		let entry = desktop_entry(Path::new("/home/me/My Apps/prybar"));
		assert!(entry.contains("\nExec=\"/home/me/My Apps/prybar\" %F\n"));
	}
}
