//! Search paths in mount order; GMod's order is from wiki.facepunch.com/gmod/Filesystem.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::games::{Games, Target, is_numbered_vpk_part};
use crate::package::Package;
use crate::steam::{entries, find_file};
use crate::vdf::{self, ParseOptions};

const GMOD_APP_ID: u32 = 4000;
/// In gmod's gameinfo.txt order; other VPKs follow.
const GMOD_ENGINE_VPKS: [&str; 4] = [
	"hl2_textures_dir.vpk",
	"hl2_sound_vo_english_dir.vpk",
	"hl2_sound_misc_dir.vpk",
	"hl2_misc_dir.vpk",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountKind {
	Folder,
	Package,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
	pub path: PathBuf,
	pub kind: MountKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
	pub mount: PathBuf,
	pub kind: MountKind,
	/// Original letter case.
	pub path: String,
	pub size: u64,
}

impl Source {
	pub fn file(&self) -> PathBuf {
		match self.kind {
			MountKind::Package => self.mount.clone(),
			MountKind::Folder => self.mount.join(&self.path),
		}
	}
}

/// The first source is the one the game loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
	pub path: String,
	pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Copy)]
struct Hit {
	mount: u32,
	entry: u32,
}

#[derive(Debug)]
enum Content {
	Package(Arc<Package>),
	Folder(Vec<(String, u64)>),
	Failed(String),
}

#[derive(Debug)]
pub struct Vfs {
	pub mounts: Vec<Mount>,
	contents: Vec<Content>,
	/// Lower-case paths with forward slashes.
	files: HashMap<String, Vec<Hit>>,
}

pub fn read_file(path: &Path, max_size: u64) -> Result<Vec<u8>, String> {
	let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
	if size > max_size {
		return Err(format!("{} is too large", path.display()));
	}
	std::fs::read(path).map_err(|e| e.to_string())
}

pub fn mounts(games: &Games, target: &Target) -> Vec<Mount> {
	let mut plan = Plan::default();
	if target.app.id == GMOD_APP_ID {
		gmod_mounts(games, target, &mut plan);
	} else {
		source_mounts(&base_dir(games, target), &target.game_dir, &mut plan);
	}
	plan.mounts
}

impl Vfs {
	/// `progress` gets the share done and can stop the build.
	pub fn build(
		mounts: Vec<Mount>,
		mut progress: impl FnMut(f32) -> ControlFlow<()>,
	) -> Option<Self> {
		let mut contents = Vec::with_capacity(mounts.len());
		let mut files: HashMap<String, Vec<Hit>> = HashMap::new();
		for (index, mount) in mounts.iter().enumerate() {
			if progress(index as f32 / mounts.len().max(1) as f32).is_break() {
				return None;
			}
			let content = match mount.kind {
				MountKind::Package => match Package::open(&mount.path) {
					Ok(package) => Content::Package(Arc::new(package)),
					Err(e) => Content::Failed(e.to_string()),
				},
				MountKind::Folder => {
					let mut list = Vec::new();
					list_folder(&mount.path, "", &mut list);
					Content::Folder(list)
				}
			};
			let mut add = |path: &str, entry: usize| {
				let hits = files.entry(normalize(path)).or_default();
				// A package can list a path twice; the first wins.
				if hits.last().is_none_or(|hit| hit.mount as usize != index) {
					hits.push(Hit {
						mount: index as u32,
						entry: entry as u32,
					});
				}
			};
			match &content {
				Content::Package(package) => {
					for (i, entry) in package.entries.iter().enumerate() {
						add(&entry.path, i);
					}
				}
				Content::Folder(list) => {
					for (i, (path, _)) in list.iter().enumerate() {
						add(path, i);
					}
				}
				Content::Failed(_) => {}
			}
			contents.push(content);
		}
		let _ = progress(1.0);
		Some(Self {
			mounts,
			contents,
			files,
		})
	}

	pub fn failures(&self) -> impl Iterator<Item = (&Path, &str)> {
		self.mounts
			.iter()
			.zip(&self.contents)
			.filter_map(|(mount, content)| match content {
				Content::Failed(e) => Some((mount.path.as_path(), e.as_str())),
				_ => None,
			})
	}

	pub fn file_count(&self) -> usize {
		self.files.len()
	}

	pub fn find(&self, path: &str) -> Option<Found> {
		let key = normalize(path);
		let hits = self.files.get(&key)?;
		Some(self.found(hits))
	}

	/// Reads the copy the game loads.
	pub fn read(&self, path: &str, max_size: u64) -> Option<(Source, Result<Vec<u8>, String>)> {
		let hit = *self.files.get(&normalize(path))?.first()?;
		let source = self.source(hit);
		let data = match &self.contents[hit.mount as usize] {
			Content::Package(package) => package
				.read(&package.entries[hit.entry as usize], max_size)
				.map_err(|e| e.to_string()),
			_ => read_file(&source.file(), max_size),
		};
		Some((source, data))
	}

	/// Sorted by path.
	pub fn search(&self, needle: &str, limit: usize) -> Vec<Found> {
		let needle = normalize(needle);
		let mut keys: Vec<&String> = self
			.files
			.keys()
			.filter(|key| key.contains(&needle))
			.collect();
		keys.sort_unstable();
		keys.truncate(limit);
		keys.into_iter()
			.map(|key| self.found(&self.files[key]))
			.collect()
	}

	fn found(&self, hits: &[Hit]) -> Found {
		let sources: Vec<Source> = hits.iter().map(|hit| self.source(*hit)).collect();
		Found {
			path: sources[0].path.clone(),
			sources,
		}
	}

	fn source(&self, hit: Hit) -> Source {
		let mount = &self.mounts[hit.mount as usize];
		let (path, size) = match &self.contents[hit.mount as usize] {
			Content::Package(package) => {
				let entry = &package.entries[hit.entry as usize];
				(entry.path.clone(), entry.size)
			}
			Content::Folder(list) => list[hit.entry as usize].clone(),
			Content::Failed(_) => unreachable!("failed mounts have no files"),
		};
		Source {
			mount: mount.path.clone(),
			kind: mount.kind,
			path,
			size,
		}
	}
}

pub fn normalize(path: &str) -> String {
	let path = path.trim().replace('\\', "/").to_lowercase();
	path.split('/')
		.filter(|part| !part.is_empty() && *part != ".")
		.collect::<Vec<_>>()
		.join("/")
}

#[derive(Default)]
struct Plan {
	mounts: Vec<Mount>,
	seen: HashSet<PathBuf>,
}

impl Plan {
	fn add(&mut self, path: PathBuf, kind: MountKind) {
		if self.seen.insert(path.clone()) {
			self.mounts.push(Mount { path, kind });
		}
	}

	fn folder(&mut self, path: Option<PathBuf>) {
		if let Some(path) = path.filter(|p| p.is_dir()) {
			self.add(path, MountKind::Folder);
		}
	}

	fn package(&mut self, path: Option<PathBuf>) {
		if let Some(path) = path.filter(|p| p.is_file()) {
			self.add(path, MountKind::Package);
		}
	}

	/// `_dir` and single-file VPKs, by name.
	fn vpks_in(&mut self, dir: &Path) {
		for entry in sorted_entries(dir) {
			if !entry.is_dir && entry.lower.ends_with(".vpk") && !is_numbered_vpk_part(&entry.lower)
			{
				self.add(entry.path, MountKind::Package);
			}
		}
	}
}

/// Root for relative gameinfo paths: the game's folder, or the base game's for a mod.
fn base_dir(games: &Games, target: &Target) -> PathBuf {
	if target.app.is_mod
		&& let Some(base) = games.app(target.app.id).filter(|app| !app.is_mod)
	{
		return base.install_path.clone();
	}
	target.app.install_path.clone()
}

fn source_mounts(base_dir: &Path, game_dir: &Path, plan: &mut Plan) {
	let Some(file) = find_file(game_dir, "gameinfo.txt") else {
		plan.folder(Some(game_dir.to_path_buf()));
		return;
	};
	let Ok(root) = vdf::parse_file(&file, ParseOptions::RAW) else {
		plan.folder(Some(game_dir.to_path_buf()));
		return;
	};
	let paths = root
		.find("GameInfo")
		.and_then(|n| n.find("FileSystem"))
		.and_then(|n| n.find("SearchPaths"));
	let Some(paths) = paths else {
		plan.folder(Some(game_dir.to_path_buf()));
		return;
	};
	for child in paths.children() {
		let is_game = child
			.key
			.split('+')
			.any(|id| id.trim().eq_ignore_ascii_case("game"));
		if let Some(value) = child.as_str().filter(|_| is_game) {
			search_path(value, base_dir, game_dir, plan);
		}
	}
}

fn search_path(value: &str, base_dir: &Path, game_dir: &Path, plan: &mut Plan) {
	let value = value.trim().replace('\\', "/");
	let (root, rest) = if let Some(rest) = strip_prefix_ci(&value, "|gameinfo_path|") {
		(game_dir.to_path_buf(), rest)
	} else if let Some(rest) = strip_prefix_ci(&value, "|all_source_engine_paths|") {
		(base_dir.to_path_buf(), rest)
	} else if value.starts_with('/') {
		(PathBuf::from("/"), value.as_str())
	} else {
		(base_dir.to_path_buf(), value.as_str())
	};

	if rest == "*" || rest.ends_with("/*") {
		// custom/*: every folder and VPK in it, in name order.
		let Some(dir) = resolve_ci(&root, rest.trim_end_matches('*')) else {
			return;
		};
		for entry in sorted_entries(&dir) {
			if entry.is_dir {
				plan.add(entry.path, MountKind::Folder);
			} else if entry.lower.ends_with(".vpk") && !is_numbered_vpk_part(&entry.lower) {
				plan.add(entry.path, MountKind::Package);
			}
		}
	} else if let Some(stem) = rest
		.len()
		.checked_sub(4)
		.filter(|&at| rest.is_char_boundary(at) && rest[at..].eq_ignore_ascii_case(".vpk"))
		.map(|at| &rest[..at])
	{
		// The engine turns pak01.vpk into pak01_dir.vpk.
		let dir_file = resolve_ci(&root, &format!("{stem}_dir.vpk"));
		plan.package(dir_file.or_else(|| resolve_ci(&root, rest)));
	} else {
		plan.folder(resolve_ci(&root, rest));
	}
}

fn gmod_mounts(games: &Games, target: &Target, plan: &mut Plan) {
	let game = &target.game_dir;
	let base = &target.app.install_path;
	let addons = game.join("addons");
	let disabled = gmod_disabled_addons(game);

	// Legacy addons, then GMAs inside them, then workshop GMAs.
	let legacy: Vec<PathBuf> = sorted_entries(&addons)
		.into_iter()
		.filter(|e| e.is_dir)
		.map(|e| e.path)
		.collect();
	for dir in &legacy {
		plan.add(dir.clone(), MountKind::Folder);
	}
	for dir in &legacy {
		for entry in sorted_entries(dir) {
			if !entry.is_dir && entry.lower.ends_with(".gma") {
				plan.add(entry.path, MountKind::Package);
			}
		}
	}
	let mut workshop = Vec::new();
	for entry in sorted_entries(&addons) {
		if !entry.is_dir
			&& entry.lower.ends_with(".gma")
			&& !disabled.contains(&gma_id(&entry.lower))
		{
			workshop.push(entry.path);
		}
	}
	for library in &games.libraries {
		let content = library.join(format!("steamapps/workshop/content/{GMOD_APP_ID}"));
		let mut items = sorted_entries(&content);
		items.sort_by_key(|item| item.lower.parse::<u64>().unwrap_or(u64::MAX));
		for item in items {
			if !item.is_dir || disabled.contains(&item.lower) {
				continue;
			}
			for entry in sorted_entries(&item.path) {
				if !entry.is_dir && entry.lower.ends_with(".gma") {
					workshop.push(entry.path);
				}
			}
		}
	}
	for gma in workshop {
		plan.add(gma, MountKind::Package);
	}

	plan.folder(resolve_ci(game, "overrides"));
	plan.package(find_file(game, "garrysmod_dir.vpk"));
	plan.folder(Some(game.clone()));
	// GMod mounts every gamemode's content, not only the active one; the spot after garrysmod/ is a guess.
	for gamemode in sorted_entries(&game.join("gamemodes")) {
		if gamemode.is_dir {
			plan.folder(resolve_ci(&gamemode.path, "content"));
		}
	}

	let engine = resolve_ci(base, "sourceengine");
	if let Some(engine) = &engine {
		for name in GMOD_ENGINE_VPKS {
			plan.package(find_file(engine, name));
		}
		plan.vpks_in(engine);
	}
	plan.folder(engine);
	if let Some(platform) = resolve_ci(base, "platform") {
		plan.package(find_file(&platform, "platform_misc_dir.vpk"));
		plan.folder(Some(platform));
	}

	for folder in gmod_mounted_games(games, game) {
		match folder {
			Mounted::Game(other) => source_mounts(&base_dir(games, &other), &other.game_dir, plan),
			Mounted::Folder(dir) => {
				plan.vpks_in(&dir);
				plan.folder(Some(dir));
			}
		}
	}

	plan.folder(resolve_ci(game, "download"));
	plan.package(find_file(game, "fallbacks_dir.vpk"));
	plan.folder(resolve_ci(game, "fallbacks"));
}

enum Mounted {
	Game(Box<Target>),
	Folder(PathBuf),
}

/// Games ticked in the menu (cfg/mountdepots.txt), then folders from cfg/mount.cfg.
fn gmod_mounted_games(games: &Games, game_dir: &Path) -> Vec<Mounted> {
	let mut mounted = Vec::new();
	let cfg = resolve_ci(game_dir, "cfg");
	let read = |name: &str| {
		let file = find_file(cfg.as_deref()?, name)?;
		vdf::parse_file(&file, ParseOptions::RAW).ok()
	};
	if let Some(root) = read("mountdepots.txt") {
		for depot in root.children().iter().flat_map(|n| n.children()) {
			if depot.as_str() != Some("1") {
				continue;
			}
			let found = games.targets().find(|t| {
				t.app.id != GMOD_APP_ID
					&& t.game_dir
						.file_name()
						.is_some_and(|name| name.eq_ignore_ascii_case(&depot.key))
			});
			if let Some(target) = found {
				mounted.push(Mounted::Game(Box::new(target)));
			}
		}
	}
	if let Some(root) = read("mount.cfg") {
		for entry in root.children().iter().flat_map(|n| n.children()) {
			if let Some(path) = entry.as_str().map(PathBuf::from).filter(|p| p.is_dir()) {
				mounted.push(Mounted::Folder(path));
			}
		}
	}
	mounted
}

/// Workshop ids switched off in the menu.
fn gmod_disabled_addons(game_dir: &Path) -> HashSet<String> {
	let file = resolve_ci(game_dir, "cfg/addonnomount.txt");
	let Some(root) = file.and_then(|f| vdf::parse_file(&f, ParseOptions::RAW).ok()) else {
		return HashSet::new();
	};
	root.children()
		.iter()
		.flat_map(|n| n.children())
		.filter_map(|n| n.as_str().map(str::to_owned))
		.collect()
}

/// Old workshop GMAs in addons/ are named `title_<id>.gma`.
fn gma_id(lower_name: &str) -> String {
	let stem = lower_name.trim_end_matches(".gma");
	stem.rsplit('_').next().unwrap_or(stem).to_owned()
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
	let head = text.get(..prefix.len())?;
	head.eq_ignore_ascii_case(prefix)
		.then(|| &text[prefix.len()..])
}

/// Case-insensitive, as on Windows.
pub(crate) fn resolve_ci(root: &Path, relative: &str) -> Option<PathBuf> {
	let mut path = root.to_path_buf();
	for part in relative.split('/') {
		match part {
			"" | "." => {}
			".." => {
				path.pop();
			}
			_ => {
				let exact = path.join(part);
				if exact.exists() {
					path = exact;
					continue;
				}
				let lower = part.to_lowercase();
				path = entries(&path).into_iter().find(|e| e.lower == lower)?.path;
			}
		}
	}
	path.exists().then_some(path)
}

fn sorted_entries(dir: &Path) -> Vec<crate::steam::DirEntry> {
	let mut list = entries(dir);
	list.sort_by(|a, b| a.lower.cmp(&b.lower));
	list
}

/// Doesn't follow symlinked folders.
fn list_folder(dir: &Path, prefix: &str, out: &mut Vec<(String, u64)>) {
	let Ok(read) = std::fs::read_dir(dir) else {
		return;
	};
	for entry in read.flatten() {
		let Ok(kind) = entry.file_type() else {
			continue;
		};
		let name = entry.file_name().to_string_lossy().into_owned();
		let relative = if prefix.is_empty() {
			name
		} else {
			format!("{prefix}/{name}")
		};
		if kind.is_dir() {
			list_folder(&entry.path(), &relative, out);
		} else if let Ok(meta) = std::fs::metadata(entry.path())
			&& meta.is_file()
		{
			out.push((relative, meta.len()));
		}
	}
}

#[cfg(test)]
mod tests;
