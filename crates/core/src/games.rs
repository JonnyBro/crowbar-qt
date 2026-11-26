//! Games from Steam, adjusted by the settings.

use std::path::{Path, PathBuf};

use crate::paths::{AppPaths, clean_path};
use crate::settings::Settings;
use crate::steam::{self, App, CompatTool, Installation};

#[derive(Debug, Clone, Default)]
pub struct Games {
	pub apps: Vec<App>,
	pub libraries: Vec<PathBuf>,
	/// Every Proton build found, for choosing one in the settings.
	pub compat_tools: Vec<CompatTool>,
	pub steam_found: bool,
}

/// One game folder of one game; the thing every tool works on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
	pub app: App,
	pub game_dir: PathBuf,
}

impl Games {
	pub fn scan(settings: &Settings, paths: &AppPaths) -> Self {
		Self::from_installations(
			steam::scan_all(),
			settings,
			&paths.data_dir().join("prefixes"),
		)
	}

	/// `prefixes_dir` holds Wine prefixes for tools that need Proton.
	pub fn from_installations(
		installations: Vec<Installation>,
		settings: &Settings,
		prefixes_dir: &Path,
	) -> Self {
		let mut games = Self {
			steam_found: !installations.is_empty(),
			..Self::default()
		};
		let first_root = installations.first().map(|i| i.root.clone());

		for installation in installations {
			for mut app in installation.apps {
				if !app.is_source_game() {
					continue;
				}
				if let Some(tools) = settings.tools_override(app.id) {
					// One prefix per game keeps its Wine settings apart from the others.
					let prefix = prefixes_dir.join(app.id.to_string());
					steam::override_tools(&mut app, tools, &installation.compat_tools, &prefix);
				}
				games.apps.push(app);
			}
			games.libraries.extend(installation.libraries);
			games.compat_tools.extend(installation.compat_tools);
		}

		// Added by hand after the scan, so they borrow tools that were overridden above.
		let installed = games.apps.clone();
		for folder in &settings.custom_folders {
			let folder = clean_path(folder);
			let listed = installed.iter().any(|app| app.game_dirs.contains(&folder));
			if listed {
				continue;
			}
			if let Some(mut app) = steam::app_for_game_dir(&folder, &installed) {
				if app.steam_root.is_none() {
					app.steam_root.clone_from(&first_root);
				}
				games.apps.push(app);
			}
		}
		steam::sort_by_name(&mut games.apps);

		// A Proton chosen in the settings runs the tools of every game that uses Proton.
		let chosen = settings
			.proton
			.as_deref()
			.and_then(|name| games.compat_tools.iter().find(|tool| tool.name == name))
			.cloned();
		if let Some(chosen) = chosen {
			for app in games
				.apps
				.iter_mut()
				.filter(|app| app.proton_path.is_some())
			{
				app.proton_path = Some(chosen.path.clone());
				app.compat_tool_name = Some(chosen.name.clone());
			}
		}
		games
	}

	/// Every game folder of every game, in display order.
	pub fn targets(&self) -> impl Iterator<Item = Target> + '_ {
		self.apps.iter().flat_map(|app| {
			app.game_dirs.iter().map(|dir| Target {
				app: app.clone(),
				game_dir: dir.clone(),
			})
		})
	}

	pub fn target(&self, game_dir: &Path) -> Option<Target> {
		self.targets().find(|target| target.game_dir == game_dir)
	}

	/// Mods and added folders only when no installed app has the id.
	pub fn app(&self, app_id: u32) -> Option<&App> {
		self.apps
			.iter()
			.find(|app| app.id == app_id && !app.is_mod)
			.or_else(|| self.apps.iter().find(|app| app.id == app_id))
	}

	/// The saved folder if it exists, else the first game that runs the tools, else the first.
	pub fn current(&self, saved: Option<&Path>) -> Option<Target> {
		saved
			.and_then(|dir| self.target(dir))
			.or_else(|| self.targets().find(|t| t.app.tools_path.is_some()))
			.or_else(|| self.targets().next())
	}
}

/// `pak01_003.vpk`: a data part, opened through its `_dir.vpk`.
pub(crate) fn is_numbered_vpk_part(lower_name: &str) -> bool {
	lower_name
		.strip_suffix(".vpk")
		.and_then(|stem| stem.rsplit_once('_'))
		.is_some_and(|(_, n)| n.len() == 3 && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::steam::Platform;

	fn write(path: &Path, content: &str) {
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		std::fs::write(path, content).unwrap();
	}

	fn tool(name: &str, path: &Path) -> CompatTool {
		CompatTool {
			name: name.into(),
			display_name: name.into(),
			path: path.to_path_buf(),
		}
	}

	fn app(id: u32, name: &str, dir: &Path) -> App {
		App {
			id,
			name: name.into(),
			install_path: dir.to_path_buf(),
			game_dirs: vec![dir.join("game")],
			..App::default()
		}
	}

	#[test]
	fn overrides_custom_folders_and_proton() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path().canonicalize().unwrap();
		write(&dir.join("wintools/studiomdl.exe"), "");
		write(
			&dir.join("custom/gameinfo.txt"),
			"GameInfo { game \"Added\" FileSystem { SteamAppId 440 } }",
		);

		let windows = App {
			platform: Platform::Windows,
			proton_path: Some(dir.join("own-proton")),
			compat_tool_name: Some("own".into()),
			..app(4000, "Garry's Mod", &dir.join("gmod"))
		};
		let installation = Installation {
			root: dir.join("Steam"),
			libraries: vec![dir.join("Steam")],
			compat_tools: vec![
				tool("proton_experimental", &dir.join("exp")),
				tool("proton_9", &dir.join("p9")),
			],
			apps: vec![
				app(440, "Team Fortress 2", &dir.join("tf2")),
				windows,
				App {
					game_dirs: Vec::new(),
					..app(1, "Not Source", &dir.join("other"))
				},
			],
		};

		let mut settings = Settings {
			proton: Some("proton_9".into()),
			custom_folders: vec![dir.join("custom/"), dir.join("tf2/game")],
			..Settings::default()
		};
		settings.update_game(440, |g| g.tools = Some(dir.join("wintools")));

		let games = Games::from_installations(vec![installation], &settings, &dir.join("prefixes"));
		let names: Vec<_> = games.apps.iter().map(|a| a.name.as_str()).collect();
		assert_eq!(names, ["Added", "Garry's Mod", "Team Fortress 2"]);

		let tf2 = games
			.apps
			.iter()
			.find(|a| a.name == "Team Fortress 2")
			.unwrap();
		assert!(tf2.tools_overridden);
		assert_eq!(tf2.compat_data_path, Some(dir.join("prefixes/440")));
		// The chosen Proton wins over the one picked for the overridden tools.
		assert_eq!(tf2.compat_tool_name.as_deref(), Some("proton_9"));

		let added = games.apps.iter().find(|a| a.name == "Added").unwrap();
		assert_eq!(added.tools_path, Some(dir.join("wintools")));
		assert_eq!(added.steam_root, Some(dir.join("Steam")));

		let gmod = games.apps.iter().find(|a| a.id == 4000).unwrap();
		assert_eq!(gmod.proton_path, Some(dir.join("p9")));

		assert_eq!(games.app(440).unwrap().name, "Team Fortress 2");
	}

	#[test]
	fn current_prefers_saved_then_tools() {
		let dir = Path::new("/games");
		let games = Games {
			apps: vec![
				app(1, "A", &dir.join("a")),
				App {
					tools_path: Some(dir.join("b/bin")),
					..app(2, "B", &dir.join("b"))
				},
			],
			..Games::default()
		};
		let current = |saved: Option<&Path>| games.current(saved).map(|t| t.game_dir);
		assert_eq!(current(None), Some(dir.join("b/game")));
		assert_eq!(current(Some(&dir.join("a/game"))), Some(dir.join("a/game")));
		assert_eq!(current(Some(Path::new("/gone"))), Some(dir.join("b/game")));
		assert_eq!(Games::default().current(None), None);
	}
}
