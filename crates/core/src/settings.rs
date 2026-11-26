use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::paths::AppPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
	#[default]
	System,
	Dark,
	Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
	#[default]
	System,
	English,
	Russian,
}

impl Language {
	/// The rust-i18n locale this setting stands for.
	pub fn locale(self) -> &'static str {
		match self {
			Self::English => "en",
			Self::Russian => "ru",
			Self::System => match sys_locale::get_locale() {
				Some(l) if l.starts_with("ru") => "ru",
				_ => "en",
			},
		}
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
	pub language: Language,
	pub theme: Theme,
	/// Scheme names for dark and light; empty means the default.
	#[serde(skip_serializing_if = "String::is_empty")]
	pub dark_scheme: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	pub light_scheme: String,
	/// Proton for every game's tools; `None` keeps each game's own.
	pub proton: Option<String>,
	/// Extra environment for the game's tools, one `NAME=value` or `-NAME` per line.
	#[serde(skip_serializing_if = "String::is_empty")]
	pub tool_environment: String,
	/// Command that opens files linked from task logs; empty means the system default.
	#[serde(skip_serializing_if = "String::is_empty")]
	pub editor: String,
	/// The game folder the window works with.
	pub current_game: Option<PathBuf>,
	/// Game folders added by hand (a gameinfo.txt folder outside Steam's lists).
	#[serde(skip_serializing_if = "Vec::is_empty")]
	pub custom_folders: Vec<PathBuf>,
	/// Use the game's gmad and vpk instead of Prybar's writers.
	#[serde(skip_serializing_if = "std::ops::Not::not")]
	pub use_game_tools: bool,
	/// Don't ask GitHub for a newer release at start.
	#[serde(skip_serializing_if = "std::ops::Not::not")]
	pub skip_update_check: bool,
	/// Per app id; TOML keys are strings.
	#[serde(skip_serializing_if = "BTreeMap::is_empty")]
	pub games: BTreeMap<String, GameSettings>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GameSettings {
	/// A tools folder chosen by the user instead of the one in the game folder.
	pub tools: Option<PathBuf>,
	#[serde(skip_serializing_if = "String::is_empty")]
	pub launch_options: String,
}

impl GameSettings {
	fn is_empty(&self) -> bool {
		self.tools.is_none() && self.launch_options.is_empty()
	}
}

impl Settings {
	pub fn game(&self, app_id: u32) -> Option<&GameSettings> {
		self.games.get(&app_id.to_string())
	}

	/// Changes the settings of one game, dropping the entry once it holds nothing.
	pub fn update_game(&mut self, app_id: u32, change: impl FnOnce(&mut GameSettings)) {
		let key = app_id.to_string();
		let game = self.games.entry(key.clone()).or_default();
		change(game);
		if game.is_empty() {
			self.games.remove(&key);
		}
	}

	pub fn tools_override(&self, app_id: u32) -> Option<&Path> {
		self.game(app_id).and_then(|game| game.tools.as_deref())
	}

	/// Defaults when the file doesn't exist yet.
	pub fn load(paths: &AppPaths) -> Result<Self> {
		let path = paths.settings_file();
		match std::fs::read_to_string(&path) {
			Ok(text) => {
				toml::from_str(&text).map_err(|source| Error::SettingsParse { path, source })
			}
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
			Err(e) => Err(e).at(path),
		}
	}

	/// Written to a temporary file first, so a crash never leaves half a file.
	pub fn save(&self, paths: &AppPaths) -> Result<()> {
		let path = paths.settings_file();
		let text = toml::to_string_pretty(self)?;
		let tmp = path.with_extension("toml.tmp");
		std::fs::write(&tmp, text).at(&tmp)?;
		std::fs::rename(&tmp, &path).at(&path)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn paths() -> (tempfile::TempDir, AppPaths) {
		let root = tempfile::tempdir().unwrap();
		let paths = AppPaths::under(root.path());
		paths.ensure_exist().unwrap();
		(root, paths)
	}

	#[test]
	fn missing_file_gives_defaults() {
		let (_root, paths) = paths();
		assert_eq!(Settings::load(&paths).unwrap(), Settings::default());
	}

	#[test]
	fn save_and_load_round_trip() {
		let (_root, paths) = paths();
		let mut settings = Settings {
			language: Language::Russian,
			theme: Theme::Dark,
			dark_scheme: "Nord".into(),
			proton: Some("proton_9".into()),
			custom_folders: vec!["/games/mymod".into()],
			..Settings::default()
		};
		settings.update_game(4000, |g| {
			g.tools = Some("/tools/bin".into());
			g.launch_options = "-console".into();
		});
		settings.save(&paths).unwrap();
		assert_eq!(Settings::load(&paths).unwrap(), settings);
	}

	#[test]
	fn unknown_keys_and_missing_keys_are_fine() {
		let (_root, paths) = paths();
		std::fs::write(paths.settings_file(), "theme = \"light\"\nfuture_key = 1\n").unwrap();
		let settings = Settings::load(&paths).unwrap();
		assert_eq!(settings.theme, Theme::Light);
		assert_eq!(settings.language, Language::System);
	}

	#[test]
	fn empty_game_entries_are_dropped() {
		let mut settings = Settings::default();
		settings.update_game(440, |g| g.tools = Some("/x".into()));
		assert_eq!(settings.tools_override(440), Some(Path::new("/x")));
		settings.update_game(440, |g| g.tools = None);
		assert!(settings.games.is_empty());
	}

	#[test]
	fn broken_file_is_an_error() {
		let (_root, paths) = paths();
		std::fs::write(paths.settings_file(), "theme = [").unwrap();
		assert!(matches!(
			Settings::load(&paths),
			Err(Error::SettingsParse { .. })
		));
	}
}
