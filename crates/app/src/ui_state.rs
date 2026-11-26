//! What the pages remember between runs: choices and options, the open page.
//! Paths are not remembered; every run starts with empty path fields.

use std::collections::BTreeMap;
use std::path::Path;

use gpui_kit::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
	/// Size and place of the window when it was last closed.
	pub window: Option<SavedBounds>,
	pub page: String,
	pub tasks_open: bool,
	pub compile: CompilePrefs,
	pub packages: PackagesPrefs,
	pub pack: PackPrefs,
	pub download: DownloadPrefs,
	/// Column widths per table, see `views::columns`.
	pub columns: BTreeMap<String, Vec<f32>>,
}

impl Default for UiState {
	fn default() -> Self {
		Self {
			window: None,
			page: String::new(),
			tasks_open: true,
			compile: CompilePrefs::default(),
			packages: PackagesPrefs::default(),
			pack: PackPrefs::default(),
			download: DownloadPrefs::default(),
			columns: BTreeMap::new(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedBounds {
	pub x: f32,
	pub y: f32,
	pub width: f32,
	pub height: f32,
	pub maximized: bool,
}

impl SavedBounds {
	pub fn from_window(bounds: WindowBounds) -> Self {
		let (b, maximized) = match bounds {
			WindowBounds::Windowed(b) => (b, false),
			WindowBounds::Maximized(b) | WindowBounds::Fullscreen(b) => (b, true),
		};
		Self {
			x: b.origin.x.as_f32(),
			y: b.origin.y.as_f32(),
			width: b.size.width.as_f32(),
			height: b.size.height.as_f32(),
			maximized,
		}
	}

	pub fn to_window(self) -> WindowBounds {
		let b = Bounds::new(
			point(px(self.x), px(self.y)),
			size(px(self.width), px(self.height)),
		);
		if self.maximized {
			WindowBounds::Maximized(b)
		} else {
			WindowBounds::Windowed(b)
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
	pub output: OutputMode,
	pub subfolder: String,
	pub no_p4: bool,
	pub verbose: bool,
	pub view_after: bool,
	pub extra: String,
	/// Rebuild QCs from the list when their files change.
	pub watch: bool,
}

impl Default for CompilePrefs {
	fn default() -> Self {
		Self {
			output: OutputMode::Beside,
			subfolder: "compiled".into(),
			no_p4: true,
			verbose: false,
			view_after: false,
			extra: String::new(),
			watch: false,
		}
	}
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PackagesPrefs {
	pub output: OutputMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PackPrefs {
	pub gma: bool,
	pub output: OutputMode,
	pub multi: bool,
	/// Size of a multi-file VPK's chunk, in megabytes.
	pub chunk_mb: u64,
	pub warn_invalid: bool,
	pub extra: String,
}

impl Default for PackPrefs {
	fn default() -> Self {
		Self {
			gma: false,
			output: OutputMode::default(),
			multi: false,
			chunk_mb: prybar_core::vpk_writer::DEFAULT_CHUNK_MB,
			warn_invalid: false,
			extra: String::new(),
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadPrefs {
	pub with_title: bool,
	pub convert: bool,
}

impl Default for DownloadPrefs {
	fn default() -> Self {
		Self {
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
