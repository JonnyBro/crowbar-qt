//! Everything prybar does without a window: finding games, reading packages,
//! running the game's tools. The app and the command line both sit on top of it.

rust_i18n::i18n!("locales", fallback = "en");

pub mod app_update;
pub mod decompile;
pub mod engine;
pub mod error;
mod fsutil;
pub mod games;
pub mod gma_whitelist;
pub mod gma_writer;
pub mod inspect;
pub mod launcher;
pub mod mdl;
pub mod package;
pub mod packer;
pub mod paths;
pub mod process;
pub mod publisher;
pub mod runner;
pub mod settings;
pub mod steam;
pub mod studiomdl;
pub mod vdf;
pub mod vfs;
pub mod vpk_writer;
pub mod vtf;
pub mod workshop;

pub use engine::{Command, Engine, Event};
pub use error::{Error, Result};
pub use games::{Games, Target};
pub use paths::AppPaths;
pub use settings::Settings;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// rust_i18n looks keys up in the calling crate's own file only, and shows a
/// missing one as the bare key.
#[cfg(test)]
mod locale_keys {
	use std::path::Path;

	fn used_keys(dir: &Path, out: &mut Vec<String>) {
		for entry in std::fs::read_dir(dir).unwrap().flatten() {
			let path = entry.path();
			if path.is_dir() {
				used_keys(&path, out);
			} else if path.extension().is_some_and(|e| e == "rs") {
				let text = std::fs::read_to_string(&path).unwrap();
				for (at, _) in text.match_indices("t!(") {
					let before = text[..at].chars().next_back();
					if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
						continue;
					}
					let rest = text[at + 3..].trim_start();
					let Some(rest) = rest.strip_prefix('"') else {
						continue;
					};
					let key = &rest[..rest.find('"').unwrap()];
					if key
						.chars()
						.all(|c| c.is_ascii_alphanumeric() || "._".contains(c))
					{
						out.push(key.to_owned());
					}
				}
			}
		}
	}

	#[test]
	fn every_key_is_in_this_crates_file() {
		let root = Path::new(env!("CARGO_MANIFEST_DIR"));
		let yml = std::fs::read_dir(root.join("locales"))
			.unwrap()
			.flatten()
			.map(|e| std::fs::read_to_string(e.path()).unwrap())
			.collect::<String>();
		let defined: Vec<&str> = yml
			.lines()
			.filter_map(|line| line.strip_suffix(':'))
			.filter(|key| !key.starts_with(' '))
			.collect();
		let mut used = Vec::new();
		used_keys(&root.join("src"), &mut used);
		used.sort();
		used.dedup();
		let missing: Vec<&String> = used
			.iter()
			.filter(|k| !defined.contains(&k.as_str()))
			.collect();
		assert!(missing.is_empty(), "{missing:?}");
	}
}
