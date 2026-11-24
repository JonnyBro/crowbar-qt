//! Steam installations: libraries, installed apps, Proton builds and the
//! Source mods in `steamapps/sourcemods`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::paths::clean_path;
use crate::vdf::{self, Node, ParseOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Platform {
	#[default]
	Linux,
	Windows,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatTool {
	/// The internal name Steam uses in `CompatToolMapping`.
	pub name: String,
	pub display_name: String,
	pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct App {
	pub id: u32,
	pub name: String,
	pub install_path: PathBuf,
	pub steam_root: Option<PathBuf>,
	pub platform: Platform,
	/// Folders with a gameinfo.txt; empty for apps that aren't Source games.
	pub game_dirs: Vec<PathBuf>,
	pub tools_path: Option<PathBuf>,
	pub compat_data_path: Option<PathBuf>,
	pub compat_tool_name: Option<String>,
	pub proton_path: Option<PathBuf>,
	/// `tools_path` was set by the user rather than found in the game folder.
	pub tools_overridden: bool,
	/// A game folder that runs on top of another app (a Source mod, or a folder
	/// added by hand); `id` is then that base app's, and `install_path` the folder itself.
	pub is_mod: bool,
	pub base_name: Option<String>,
}

impl App {
	pub fn is_source_game(&self) -> bool {
		!self.game_dirs.is_empty()
	}
}

#[derive(Debug, Clone, Default)]
pub struct Installation {
	pub root: PathBuf,
	pub libraries: Vec<PathBuf>,
	pub compat_tools: Vec<CompatTool>,
	pub apps: Vec<App>,
}

/// Every Steam root under `home` that has a `steamapps` folder, without duplicates
/// from the usual symlinks.
pub fn find_roots(home: &Path) -> Vec<PathBuf> {
	const CANDIDATES: [&str; 7] = [
		".local/share/Steam",
		".steam/steam",
		".steam/root",
		".steam/debian-installation",
		".var/app/com.valvesoftware.Steam/.local/share/Steam",
		".var/app/com.valvesoftware.Steam/data/Steam",
		"snap/steam/common/.local/share/Steam",
	];
	let mut roots = Vec::new();
	for candidate in CANDIDATES {
		if let Ok(resolved) = home.join(candidate).canonicalize()
			&& !roots.contains(&resolved)
			&& resolved.join("steamapps").is_dir()
		{
			roots.push(resolved);
		}
	}
	roots
}

pub fn scan_all() -> Vec<Installation> {
	let Some(dirs) = directories::BaseDirs::new() else {
		return Vec::new();
	};
	find_roots(dirs.home_dir())
		.iter()
		.map(|root| scan(root))
		.collect()
}

pub fn scan(root: &Path) -> Installation {
	let libraries = read_libraries(root);
	let compat_tools = read_compat_tools(root, &libraries);
	let mapping = read_compat_mapping(root);
	let mut apps = Vec::new();

	for library in &libraries {
		let steamapps = library.join("steamapps");
		for manifest in entries(&steamapps).into_iter().filter(|e| {
			!e.is_dir && e.lower.starts_with("appmanifest_") && e.lower.ends_with(".acf")
		}) {
			let Ok(document) = vdf::parse_file(&manifest.path, ParseOptions::default()) else {
				continue;
			};
			let Some(state) = document.find("AppState") else {
				continue;
			};
			let id = state.get("appid").and_then(|id| id.parse().ok()).unwrap_or(0);
			let install_dir = state.get("installdir").unwrap_or_default();
			let install_path = steamapps.join("common").join(install_dir);
			if id == 0 || install_dir.is_empty() || !install_path.is_dir() {
				continue;
			}

			let game_dirs = subdirectories(&install_path)
				.into_iter()
				.filter(|dir| find_file(dir, "gameinfo.txt").is_some())
				.collect();
			let compat_data_path = find_compat_data(&libraries, id);
			let mut app = App {
				id,
				name: state.get("name").unwrap_or_default().to_owned(),
				platform: detect_platform(state, &install_path, compat_data_path.is_some()),
				tools_path: find_tools_path(&install_path),
				steam_root: Some(root.to_path_buf()),
				install_path,
				game_dirs,
				compat_data_path,
				..App::default()
			};
			resolve_proton(&mut app, &mapping, &compat_tools);
			apps.push(app);
		}
	}

	// Source mods live in the main library only, each in a folder of its own.
	let installed = apps.clone();
	for mod_dir in subdirectories(&root.join("steamapps/sourcemods")) {
		if let Some(mut app) = app_for_game_dir(&mod_dir, &installed) {
			app.steam_root = Some(root.to_path_buf());
			apps.push(app);
		}
	}

	sort_by_name(&mut apps);
	Installation {
		root: root.to_path_buf(),
		libraries,
		compat_tools,
		apps,
	}
}

pub fn sort_by_name(apps: &mut [App]) {
	apps.sort_by_cached_key(|app| app.name.to_lowercase());
}

/// Internal name Steam uses in `CompatToolMapping` for a Valve-shipped Proton
/// folder, e.g. "Proton 9.0 (Beta)" -> "proton_9".
pub fn official_tool_name(directory_name: &str) -> Option<String> {
	match directory_name {
		"Proton - Experimental" => return Some("proton_experimental".into()),
		"Proton Hotfix" => return Some("proton_hotfix".into()),
		_ => {}
	}
	let version = directory_name.strip_prefix("Proton ")?;
	let (major, rest) = version.split_once('.')?;
	let minor: String = rest.chars().take_while(char::is_ascii_digit).collect();
	if major.is_empty() || minor.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) {
		return None;
	}
	// Steam switched from "proton_63"-style names to major-only names with Proton 7.
	if major.parse::<u32>().ok()? >= 7 {
		Some(format!("proton_{major}"))
	} else {
		Some(format!("proton_{major}{minor}"))
	}
}

/// Builds an app for a folder with a gameinfo.txt, borrowing tools, platform and
/// Proton from the installed app its SteamAppId names. `None` if the folder has
/// no gameinfo.txt.
pub fn app_for_game_dir(game_dir: &Path, installed: &[App]) -> Option<App> {
	let file = find_file(game_dir, "gameinfo.txt")?;
	// gameinfo.txt paths use backslashes that are not escapes.
	let document = vdf::parse_file(&file, ParseOptions::RAW).ok();
	let info = document.as_ref().and_then(|d| d.find("GameInfo"));
	let id = info
		.and_then(|info| info.find("FileSystem"))
		.and_then(|fs| fs.get("SteamAppId"))
		.and_then(|id| id.trim().parse().ok())
		.unwrap_or(0);
	let install_path = clean_path(&absolute(game_dir));
	let name = info
		.and_then(|info| info.get("game"))
		.filter(|name| !name.is_empty())
		.map(str::to_owned)
		.unwrap_or_else(|| file_name(&install_path));

	let mut app = App {
		id,
		name,
		game_dirs: vec![install_path.clone()],
		install_path,
		is_mod: true,
		..App::default()
	};
	if let Some(base) = installed.iter().find(|base| base.id == id && !base.is_mod) {
		app.base_name = Some(base.name.clone());
		app.steam_root.clone_from(&base.steam_root);
		app.platform = base.platform;
		app.tools_path.clone_from(&base.tools_path);
		app.compat_data_path.clone_from(&base.compat_data_path);
		app.compat_tool_name.clone_from(&base.compat_tool_name);
		app.proton_path.clone_from(&base.proton_path);
	}
	Some(app)
}

/// Points the app at a tools folder chosen by the user, e.g. the bin folder of
/// the Windows build for a native game. If those tools are Windows programs and
/// the app has no Proton of its own, one of `tools` is used with `prefix_dir`.
pub fn override_tools(app: &mut App, tools_path: &Path, tools: &[CompatTool], prefix_dir: &Path) {
	if !tools_path.is_dir() {
		return;
	}
	app.tools_path = Some(clean_path(tools_path));
	app.tools_overridden = true;
	let has_exe = has_file(tools_path, |name| name.ends_with(".exe"));
	if app.proton_path.is_some() || !has_exe {
		return;
	}

	// Valve's own builds first; otherwise the first other Valve one, then the last custom one.
	let named = |name: &str| tools.iter().find(|tool| tool.name == name);
	let Some(chosen) = named("proton_experimental")
		.or_else(|| named("proton_hotfix"))
		.or_else(|| tools.iter().find(|tool| tool.name.starts_with("proton_")))
		.or_else(|| tools.last())
	else {
		return;
	};
	app.proton_path = Some(chosen.path.clone());
	app.compat_tool_name = Some(chosen.name.clone());
	let prefix = clean_path(prefix_dir);
	if let Err(e) = std::fs::create_dir_all(&prefix) {
		tracing::warn!("creating the prefix {}: {e}", prefix.display());
	}
	app.compat_data_path = Some(prefix);
}

fn read_libraries(root: &Path) -> Vec<PathBuf> {
	let mut result = Vec::new();
	let mut seen = HashSet::new();
	let mut add = |path: &Path| {
		if let Ok(resolved) = path.canonicalize()
			&& resolved.join("steamapps").is_dir()
			&& seen.insert(resolved.clone())
		{
			result.push(resolved);
		}
	};

	add(root);
	for file in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
		let Ok(document) = vdf::parse_file(&root.join(file), ParseOptions::default()) else {
			continue;
		};
		let Some(folders) = document.find("libraryfolders") else {
			continue;
		};
		for entry in folders.children() {
			match entry.as_str() {
				None => {
					if let Some(path) = entry.get("path") {
						add(Path::new(path));
					}
				}
				// Pre-2021 format: "1" "/path/to/library" next to unrelated string keys.
				Some(path) if entry.key.parse::<u32>().is_ok() => add(Path::new(path)),
				Some(_) => {}
			}
		}
	}
	result
}

fn read_compat_tools(root: &Path, libraries: &[PathBuf]) -> Vec<CompatTool> {
	let mut tools = Vec::new();

	let mut custom_dirs = vec![
		root.join("compatibilitytools.d"),
		PathBuf::from("/usr/share/steam/compatibilitytools.d"),
		PathBuf::from("/usr/local/share/steam/compatibilitytools.d"),
	];
	if let Some(extra) = std::env::var_os("STEAM_EXTRA_COMPAT_TOOLS_PATHS") {
		custom_dirs.extend(std::env::split_paths(&extra).filter(|p| !p.as_os_str().is_empty()));
	}

	for custom_dir in custom_dirs {
		for tool_dir in subdirectories(&custom_dir) {
			let Ok(document) = vdf::parse_file(
				&tool_dir.join("compatibilitytool.vdf"),
				ParseOptions::default(),
			) else {
				continue;
			};
			let Some(entries) = descend(&document, &["compatibilitytools", "compat_tools"]) else {
				continue;
			};
			for entry in entries.children() {
				let path = clean_path(&tool_dir.join(entry.get("install_path").unwrap_or(".")));
				if path.join("proton").exists() {
					tools.push(CompatTool {
						name: entry.key.clone(),
						display_name: entry.get("display_name").unwrap_or(&entry.key).to_owned(),
						path,
					});
				}
			}
		}
	}

	for library in libraries {
		for tool_dir in subdirectories(&library.join("steamapps/common")) {
			let directory_name = file_name(&tool_dir);
			if let Some(name) = official_tool_name(&directory_name)
				&& tool_dir.join("proton").exists()
			{
				tools.push(CompatTool {
					name,
					display_name: directory_name,
					path: tool_dir,
				});
			}
		}
	}
	tools
}

fn read_compat_mapping(root: &Path) -> HashMap<u32, String> {
	let Ok(document) = vdf::parse_file(&root.join("config/config.vdf"), ParseOptions::default())
	else {
		return HashMap::new();
	};
	let path = [
		"InstallConfigStore",
		"Software",
		"Valve",
		"Steam",
		"CompatToolMapping",
	];
	descend(&document, &path)
		.map(|entries| {
			entries
				.children()
				.iter()
				.filter_map(|entry| {
					let id = entry.key.parse().ok()?;
					let name = entry.get("name").filter(|name| !name.is_empty())?;
					Some((id, name.to_owned()))
				})
				.collect()
		})
		.unwrap_or_default()
}

/// config_info is written by Proton into the prefix and lists paths inside the
/// Proton build that last used it; that is the only record when the tool was
/// picked by Steam's global default rather than a per-app mapping.
fn proton_from_config_info(compat_data_path: &Path) -> Option<PathBuf> {
	let text = std::fs::read_to_string(compat_data_path.join("config_info")).ok()?;
	text.lines().find_map(|line| {
		["/files/", "/dist/"].iter().find_map(|marker| {
			let index = line.find(marker).filter(|&i| i > 0)?;
			let proton = Path::new(&line[..index]);
			proton.join("proton").exists().then(|| proton.to_path_buf())
		})
	})
}

fn detect_platform(app_state: &Node, install_path: &Path, has_prefix: bool) -> Platform {
	let overridden = app_state
		.find("UserConfig")
		.and_then(|config| config.get("platform_override_source"))
		.is_some_and(|source| source.eq_ignore_ascii_case("windows"));
	if overridden {
		return Platform::Windows;
	}
	let linux = has_file(install_path, |name| {
		name.ends_with(".sh")
			|| name.ends_with("_linux")
			|| name.ends_with("_linux64")
			|| name.ends_with(".x86_64")
	});
	if linux {
		Platform::Linux
	} else if has_file(install_path, |name| name.ends_with(".exe")) || has_prefix {
		Platform::Windows
	} else {
		Platform::Linux
	}
}

fn find_tools_path(install_path: &Path) -> Option<PathBuf> {
	["bin", "bin/x64", "bin/win64", "bin/linux64"]
		.iter()
		.map(|sub| install_path.join(sub))
		.find(|path| has_file(path, |name| name == "studiomdl.exe" || name == "studiomdl"))
}

fn find_compat_data(libraries: &[PathBuf], app_id: u32) -> Option<PathBuf> {
	libraries
		.iter()
		.map(|library| library.join(format!("steamapps/compatdata/{app_id}")))
		.find(|path| path.join("pfx").is_dir())
}

fn resolve_proton(app: &mut App, mapping: &HashMap<u32, String>, tools: &[CompatTool]) {
	if app.platform != Platform::Windows && !mapping.contains_key(&app.id) {
		return;
	}
	let name = mapping.get(&app.id).or_else(|| mapping.get(&0)).cloned();

	if let Some(name) = &name
		&& let Some(tool) = tools.iter().find(|tool| tool.name.eq_ignore_ascii_case(name))
	{
		app.compat_tool_name = Some(tool.name.clone());
		app.proton_path = Some(tool.path.clone());
		return;
	}

	app.compat_tool_name = name;
	let Some(compat_data) = &app.compat_data_path else {
		return;
	};
	app.proton_path = proton_from_config_info(compat_data);
	if let Some(proton) = &app.proton_path
		&& app.compat_tool_name.is_none()
	{
		app.compat_tool_name = Some(file_name(proton));
	}
}

fn descend<'a>(root: &'a Node, path: &[&str]) -> Option<&'a Node> {
	path.iter().try_fold(root, |node, key| node.find(key))
}

pub(crate) struct DirEntry {
	pub path: PathBuf,
	/// The file name in lower case, for matching the way Windows tools expect.
	pub lower: String,
	pub is_dir: bool,
}

/// The entries of `dir` sorted by name; empty if it can't be read.
pub(crate) fn entries(dir: &Path) -> Vec<DirEntry> {
	let Ok(read) = std::fs::read_dir(dir) else {
		return Vec::new();
	};
	let mut result: Vec<DirEntry> = read
		.flatten()
		.map(|entry| {
			let path = entry.path();
			DirEntry {
				lower: entry.file_name().to_string_lossy().to_lowercase(),
				is_dir: path.is_dir(),
				path,
			}
		})
		.collect();
	result.sort_by(|a, b| a.path.cmp(&b.path));
	result
}

fn subdirectories(dir: &Path) -> Vec<PathBuf> {
	entries(dir)
		.into_iter()
		.filter(|e| e.is_dir)
		.map(|e| e.path)
		.collect()
}

/// Whether `dir` has a file whose lower-case name passes `matches`.
pub(crate) fn has_file(dir: &Path, matches: impl Fn(&str) -> bool) -> bool {
	entries(dir).iter().any(|e| !e.is_dir && matches(&e.lower))
}

/// The file `name` in `dir`, in any letter case.
pub(crate) fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
	entries(dir)
		.into_iter()
		.find(|e| !e.is_dir && e.lower == name)
		.map(|e| e.path)
}

pub(crate) fn file_name(path: &Path) -> String {
	path.file_name()
		.map(|name| name.to_string_lossy().into_owned())
		.unwrap_or_default()
}

fn absolute(path: &Path) -> PathBuf {
	std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn write(path: &Path, content: &str) {
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		std::fs::write(path, content).unwrap();
	}

	fn temp_root() -> (tempfile::TempDir, PathBuf) {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path().canonicalize().unwrap();
		(temp, dir)
	}

	fn find(installation: &Installation, id: u32) -> &App {
		installation.apps.iter().find(|app| app.id == id).unwrap()
	}

	#[test]
	fn official_tool_names() {
		let name = |dir| official_tool_name(dir);
		assert_eq!(name("Proton - Experimental").as_deref(), Some("proton_experimental"));
		assert_eq!(name("Proton Hotfix").as_deref(), Some("proton_hotfix"));
		assert_eq!(name("Proton 9.0 (Beta)").as_deref(), Some("proton_9"));
		assert_eq!(name("Proton 6.3").as_deref(), Some("proton_63"));
		assert_eq!(name("Proton 5.13").as_deref(), Some("proton_513"));
		assert_eq!(name("GarrysMod"), None);
		assert_eq!(name("Proton EasyAntiCheat Runtime"), None);
	}

	#[test]
	fn scan_finds_games_and_proton() {
		let (_temp, dir) = temp_root();
		let root = dir.join("Steam");
		let library = dir.join("Library");

		write(
			&root.join("steamapps/libraryfolders.vdf"),
			&format!(
				"\"libraryfolders\"\n{{\n\"0\"\n{{\n\"path\" \"{}\"\n}}\n\"1\"\n{{\n\"path\" \"{}\"\n}}\n\"2\"\n{{\n\"path\" \"/nonexistent/library\"\n}}\n}}\n",
				root.display(),
				library.display()
			),
		);
		write(
			&root.join("config/config.vdf"),
			"\"InstallConfigStore\"\n{\n\"Software\"\n{\n\"valve\"\n{\n\"Steam\"\n{\n\"CompatToolMapping\"\n{\n\"4000\"\n{\n\"name\" \"GE-Test\"\n\"config\" \"\"\n}\n}\n}\n}\n}\n}\n",
		);
		write(
			&root.join("compatibilitytools.d/GE-Test/compatibilitytool.vdf"),
			"\"compatibilitytools\"\n{\n\"compat_tools\"\n{\n\"GE-Test\" // Internal name\n{\n\"install_path\" \".\"\n\"display_name\" \"GE Test\"\n}\n}\n}\n",
		);
		write(&root.join("compatibilitytools.d/GE-Test/proton"), "");

		write(
			&library.join("steamapps/appmanifest_4000.acf"),
			"\"AppState\"\n{\n\"appid\" \"4000\"\n\"name\" \"Garry's Mod\"\n\"installdir\" \"GarrysMod\"\n}\n",
		);
		let gmod_dir = library.join("steamapps/common/GarrysMod");
		write(&gmod_dir.join("gmod.exe"), "");
		write(&gmod_dir.join("bin/studiomdl.exe"), "");
		write(&gmod_dir.join("garrysmod/gameinfo.txt"), "");
		std::fs::create_dir_all(library.join("steamapps/compatdata/4000/pfx")).unwrap();

		write(
			&root.join("steamapps/appmanifest_440.acf"),
			"\"AppState\"\n{\n\"appid\" \"440\"\n\"name\" \"Team Fortress 2\"\n\"installdir\" \"Team Fortress 2\"\n}\n",
		);
		write(&root.join("steamapps/common/Team Fortress 2/tf.sh"), "");
		write(&root.join("steamapps/common/Team Fortress 2/tf/GameInfo.txt"), "");

		write(
			&root.join("steamapps/appmanifest_10.acf"),
			"\"AppState\"\n{\n\"appid\" \"10\"\n\"name\" \"Uninstalled\"\n\"installdir\" \"Gone\"\n}\n",
		);

		let installation = scan(&root);
		assert_eq!(installation.libraries, [root.clone(), library.clone()]);
		assert_eq!(installation.apps.len(), 2);

		let gmod = find(&installation, 4000);
		assert_eq!(gmod.platform, Platform::Windows);
		assert_eq!(gmod.game_dirs, [gmod_dir.join("garrysmod")]);
		assert_eq!(gmod.tools_path, Some(gmod_dir.join("bin")));
		assert_eq!(
			gmod.compat_data_path,
			Some(library.join("steamapps/compatdata/4000"))
		);
		assert_eq!(gmod.compat_tool_name.as_deref(), Some("GE-Test"));
		assert_eq!(
			gmod.proton_path,
			Some(root.join("compatibilitytools.d/GE-Test"))
		);

		let tf2 = find(&installation, 440);
		assert_eq!(tf2.platform, Platform::Linux);
		assert_eq!(tf2.game_dirs.len(), 1);
		assert_eq!(tf2.tools_path, None);
		assert_eq!(tf2.proton_path, None);
	}

	#[test]
	fn tools_override() {
		let (_temp, dir) = temp_root();
		write(&dir.join("win/bin/studiomdl.exe"), "");
		write(&dir.join("native/bin/tool"), "");

		let tool = |name: &str, display: &str, sub: &str| CompatTool {
			name: name.into(),
			display_name: display.into(),
			path: dir.join(sub),
		};
		let tools = [
			tool("GE-Proton", "GE", "ge"),
			tool("proton_9", "Proton 9.0", "p9"),
			tool("proton_experimental", "Experimental", "exp"),
		];

		let mut app = App::default();
		override_tools(&mut app, &dir.join("missing"), &tools, &dir.join("prefix"));
		assert!(!app.tools_overridden);

		override_tools(&mut app, &dir.join("win/bin/"), &tools, &dir.join("prefix"));
		assert!(app.tools_overridden);
		assert_eq!(app.tools_path, Some(dir.join("win/bin")));
		assert_eq!(app.compat_tool_name.as_deref(), Some("proton_experimental"));
		assert_eq!(app.compat_data_path, Some(dir.join("prefix")));
		assert!(dir.join("prefix").is_dir());

		let mut native = App::default();
		override_tools(&mut native, &dir.join("native/bin"), &tools, &dir.join("prefix2"));
		assert_eq!(native.proton_path, None);

		let mut own = App {
			proton_path: Some(dir.join("mine")),
			..App::default()
		};
		override_tools(&mut own, &dir.join("win/bin"), &tools, &dir.join("prefix3"));
		assert_eq!(own.proton_path, Some(dir.join("mine")));

		let mut custom = App::default();
		override_tools(&mut custom, &dir.join("win/bin"), &tools[..1], &dir.join("prefix4"));
		assert_eq!(custom.compat_tool_name.as_deref(), Some("GE-Proton"));
	}

	#[test]
	fn source_mods() {
		let (_temp, dir) = temp_root();
		let root = dir.join("Steam");
		let base = root.join("steamapps/common/Source SDK Base 2013 Singleplayer");
		write(
			&root.join("steamapps/appmanifest_243730.acf"),
			"\"AppState\"\n{\n\"appid\" \"243730\"\n\"name\" \"Source SDK Base 2013 Singleplayer\"\n\"installdir\" \"Source SDK Base 2013 Singleplayer\"\n}\n",
		);
		write(&base.join("hl2.exe"), "");
		write(&base.join("bin/studiomdl.exe"), "");
		write(&base.join("hl2/gameinfo.txt"), "");
		std::fs::create_dir_all(root.join("steamapps/compatdata/243730/pfx")).unwrap();

		write(
			&root.join("steamapps/sourcemods/mymod/GameInfo.txt"),
			"\"GameInfo\"\n{\n\tgame \"My Mod\"\n\tFileSystem\n\t{\n\t\tSteamAppId 243730\n\t\tSearchPaths\n\t\t{\n\t\t\tgame+mod |gameinfo_path|.\n\t\t\tgame \"|appid_243730|hl2\\hl2_misc.vpk\"\n\t\t}\n\t}\n}\n",
		);
		write(
			&root.join("steamapps/sourcemods/orphan/gameinfo.txt"),
			"\"GameInfo\" { FileSystem { SteamAppId 999 } }",
		);
		std::fs::create_dir_all(root.join("steamapps/sourcemods/not a mod")).unwrap();

		let installation = scan(&root);
		let by_name = |name: &str| installation.apps.iter().find(|app| app.name == name).unwrap();
		let mymod = by_name("My Mod");
		assert!(mymod.is_mod);
		assert_eq!(mymod.id, 243730);
		assert_eq!(
			mymod.base_name.as_deref(),
			Some("Source SDK Base 2013 Singleplayer")
		);
		assert_eq!(mymod.game_dirs, [root.join("steamapps/sourcemods/mymod")]);
		assert_eq!(mymod.tools_path, Some(base.join("bin")));
		assert_eq!(
			mymod.compat_data_path,
			Some(root.join("steamapps/compatdata/243730"))
		);
		assert_eq!(mymod.platform, Platform::Windows);

		let orphan = by_name("orphan");
		assert_eq!(orphan.id, 999);
		assert_eq!(orphan.tools_path, None);
		assert_eq!(orphan.base_name, None);
		assert_eq!(installation.apps.len(), 3);
		assert!(
			app_for_game_dir(
				&root.join("steamapps/sourcemods/not a mod"),
				&installation.apps
			)
			.is_none()
		);
	}

	#[test]
	fn proton_falls_back_to_prefix_config_info() {
		let (_temp, dir) = temp_root();
		let root = dir.join("Steam");
		let proton = dir.join("tools/SomeProton");

		write(&proton.join("proton"), "");
		write(
			&root.join("steamapps/appmanifest_4000.acf"),
			"\"AppState\"\n{\n\"appid\" \"4000\"\n\"name\" \"Garry's Mod\"\n\"installdir\" \"GarrysMod\"\n}\n",
		);
		write(&root.join("steamapps/common/GarrysMod/gmod.exe"), "");
		write(&root.join("steamapps/common/GarrysMod/garrysmod/gameinfo.txt"), "");
		std::fs::create_dir_all(root.join("steamapps/compatdata/4000/pfx")).unwrap();
		write(
			&root.join("steamapps/compatdata/4000/config_info"),
			&format!(
				"SomeProton\n{0}/files/share/fonts/\n{0}/files/lib/\n",
				proton.display()
			),
		);

		let installation = scan(&root);
		let gmod = find(&installation, 4000);
		assert_eq!(gmod.proton_path, Some(proton));
		assert_eq!(gmod.compat_tool_name.as_deref(), Some("SomeProton"));
	}

	#[test]
	fn find_roots_skips_symlinked_duplicates() {
		let (_temp, home) = temp_root();
		std::fs::create_dir_all(home.join(".local/share/Steam/steamapps")).unwrap();
		std::fs::create_dir_all(home.join(".steam")).unwrap();
		std::os::unix::fs::symlink(home.join(".local/share/Steam"), home.join(".steam/steam"))
			.unwrap();
		assert_eq!(find_roots(&home), [home.join(".local/share/Steam")]);
	}
}
