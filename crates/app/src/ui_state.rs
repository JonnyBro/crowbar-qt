//! What the pages remember between runs: fields, choices, the open page.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
	pub page: String,
	pub tasks_open: bool,
	pub compile: CompilePrefs,
	pub view_mdl: String,
	pub packages: PackagesPrefs,
	pub pack: PackPrefs,
	pub workshop: WorkshopPrefs,
	pub download: DownloadPrefs,
}

impl Default for UiState {
	fn default() -> Self {
		Self {
			page: String::new(),
			tasks_open: true,
			compile: CompilePrefs::default(),
			view_mdl: String::new(),
			packages: PackagesPrefs::default(),
			pack: PackPrefs::default(),
			workshop: WorkshopPrefs::default(),
			download: DownloadPrefs::default(),
		}
	}
}

/// Where output goes: next to the input or into one chosen folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
	/// Compile: the game's models folder. Unpack: a subfolder of the package's folder.
	/// Pack: the folder that holds the input folder.
	#[default]
	Beside,
	/// A subfolder of the input's folder (compile only).
	Subfolder,
	Folder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CompilePrefs {
	pub files: Vec<PathBuf>,
	pub output: OutputMode,
	pub subfolder: String,
	pub folder: String,
	pub no_p4: bool,
	pub verbose: bool,
	pub view_after: bool,
	pub extra: String,
}

impl Default for CompilePrefs {
	fn default() -> Self {
		Self {
			files: Vec::new(),
			output: OutputMode::Beside,
			subfolder: "compiled".into(),
			folder: String::new(),
			no_p4: true,
			verbose: false,
			view_after: false,
			extra: String::new(),
		}
	}
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PackagesPrefs {
	pub file: String,
	pub output: OutputMode,
	pub folder: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PackPrefs {
	pub folder: String,
	pub gma: bool,
	pub output: OutputMode,
	pub output_folder: String,
	pub multi: bool,
	pub warn_invalid: bool,
	pub extra: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkshopPrefs {
	pub content: String,
	pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadPrefs {
	pub folder: String,
	pub with_title: bool,
	pub convert: bool,
}

impl Default for DownloadPrefs {
	fn default() -> Self {
		Self {
			folder: String::new(),
			with_title: true,
			convert: true,
		}
	}
}

impl UiState {
	pub fn load(path: &Path) -> Self {
		std::fs::read(path)
			.ok()
			.and_then(|bytes| serde_json::from_slice(&bytes).ok())
			.unwrap_or_default()
	}

	pub fn save(&self, path: &Path) {
		let result = serde_json::to_vec_pretty(self)
			.map_err(std::io::Error::other)
			.and_then(|bytes| std::fs::write(path, bytes));
		if let Err(e) = result {
			tracing::warn!("could not save {}: {e}", path.display());
		}
	}
}
