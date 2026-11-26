use std::io;
use std::path::Path;

use crate::paths::clean_path;

/// Copies and deletes across file systems.
pub fn move_file(source: &Path, target: &Path) -> io::Result<()> {
	if clean_path(source) == clean_path(target) {
		return Ok(());
	}
	if let Some(parent) = target.parent() {
		std::fs::create_dir_all(parent)?;
	}
	let _ = std::fs::remove_file(target);
	match std::fs::rename(source, target) {
		Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
			std::fs::copy(source, target)?;
			std::fs::remove_file(source)
		}
		other => other,
	}
}
