use std::path::{Component, Path, PathBuf};

use directories::ProjectDirs;

use crate::error::{Error, IoContext, Result};

#[derive(Debug, Clone)]
pub struct AppPaths {
	config_dir: PathBuf,
	data_dir: PathBuf,
	cache_dir: PathBuf,
}

impl AppPaths {
	pub fn detect() -> Result<Self> {
		let dirs = ProjectDirs::from("", "", "prybar").ok_or(Error::NoPlatformDirs)?;
		Ok(Self {
			config_dir: dirs.config_dir().to_path_buf(),
			data_dir: dirs.data_dir().to_path_buf(),
			cache_dir: dirs.cache_dir().to_path_buf(),
		})
	}

	/// Puts everything under one root; used by tests.
	pub fn under(root: impl AsRef<Path>) -> Self {
		let root = root.as_ref();
		Self {
			config_dir: root.join("config"),
			data_dir: root.join("data"),
			cache_dir: root.join("cache"),
		}
	}

	pub fn ensure_exist(&self) -> Result<()> {
		for dir in [
			&self.config_dir,
			&self.data_dir,
			&self.cache_dir,
			&self.logs_dir(),
		] {
			std::fs::create_dir_all(dir).at(dir)?;
		}
		Ok(())
	}

	pub fn config_dir(&self) -> &Path {
		&self.config_dir
	}

	pub fn data_dir(&self) -> &Path {
		&self.data_dir
	}

	pub fn cache_dir(&self) -> &Path {
		&self.cache_dir
	}

	pub fn logs_dir(&self) -> PathBuf {
		self.data_dir.join("logs")
	}

	pub fn settings_file(&self) -> PathBuf {
		self.config_dir.join("settings.toml")
	}

	/// What the window remembers: page fields, the open page.
	pub fn ui_state_file(&self) -> PathBuf {
		self.config_dir.join("ui.json")
	}
}

/// Drops `.`, resolves `..` and trailing slashes without touching the disk.
pub fn clean_path(path: &Path) -> PathBuf {
	let mut result = PathBuf::new();
	for component in path.components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				if matches!(result.components().next_back(), Some(Component::Normal(_))) {
					result.pop();
				} else if !result.has_root() {
					result.push("..");
				}
			}
			other => result.push(other),
		}
	}
	result
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ensure_exist_creates_every_folder() {
		let root = tempfile::tempdir().unwrap();
		let paths = AppPaths::under(root.path());
		paths.ensure_exist().unwrap();
		assert!(paths.config_dir().is_dir());
		assert!(paths.data_dir().is_dir());
		assert!(paths.cache_dir().is_dir());
		assert!(paths.logs_dir().is_dir());
	}

	#[test]
	fn clean_path_is_lexical() {
		assert_eq!(clean_path(Path::new("/a/./b/../c/")), Path::new("/a/c"));
		assert_eq!(clean_path(Path::new("/../a")), Path::new("/a"));
		assert_eq!(clean_path(Path::new("../a/..")), Path::new(".."));
	}
}
