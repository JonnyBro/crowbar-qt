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
	/// Color scheme names for the dark and light theme; empty means the default
	/// one. The core doesn't know the schemes.
	#[serde(skip_serializing_if = "String::is_empty")]
	pub dark_scheme: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	pub light_scheme: String,
}

impl Settings {
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
		let settings = Settings {
			language: Language::Russian,
			theme: Theme::Dark,
			dark_scheme: "Nord".into(),
			light_scheme: String::new(),
		};
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
	fn broken_file_is_an_error() {
		let (_root, paths) = paths();
		std::fs::write(paths.settings_file(), "theme = [").unwrap();
		assert!(matches!(
			Settings::load(&paths),
			Err(Error::SettingsParse { .. })
		));
	}
}
