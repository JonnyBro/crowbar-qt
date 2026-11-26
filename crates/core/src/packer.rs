//! Packing a folder with the game's vpk or gmad, and the addon.json gmad needs.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rust_i18n::t;

use crate::fsutil::move_file;
use crate::launcher::{self, Command, Mode};
use crate::paths::clean_path;
use crate::steam::App;

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error(transparent)]
	Launch(#[from] launcher::Error),
	#[error("{}", t!("pack.not_folder", path = .0.display()))]
	NotFolder(PathBuf),
	#[error("{}", t!("pack.no_addon_json", path = .0.display()))]
	NoAddonJson(PathBuf),
	#[error("{}", t!("pack.io", path = .0.display(), error = .1))]
	Io(PathBuf, #[source] std::io::Error),
	#[error("{}", t!("pack.invalid_json", error = .0))]
	InvalidJson(String),
	#[error("{}", t!("pack.invalid_json_kept", error = .0))]
	InvalidJsonKept(String),
	#[error("{}", t!("pack.unsafe_update"))]
	UnsafeUpdate,
	#[error("{}", t!("pack.addon_json_exists"))]
	AddonJsonExists,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
	Vpk,
	Gma,
}

impl Format {
	pub fn tool_name(self) -> &'static str {
		match self {
			Self::Vpk => "vpk",
			Self::Gma => "gmad",
		}
	}

	pub fn extension(self) -> &'static str {
		match self {
			Self::Vpk => "vpk",
			Self::Gma => "gma",
		}
	}
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
	/// VPK only: split into `<name>_dir.vpk` plus numbered chunks (vpk -M).
	pub multi_chunk: bool,
	/// GMA only: skip files gmad does not allow instead of failing (-warninvalid).
	pub warn_invalid: bool,
	/// Passed to the packer as they are.
	pub extra: Vec<String>,
}

/// vpk always writes next to the input; [`finish_pack`] moves the result.
pub fn pack_command(
	app: &App,
	format: Format,
	folder: &Path,
	output: &Path,
	options: &Options,
	user_env: &str,
) -> Result<Command> {
	let executable = launcher::require_tool(app, format.tool_name())?;
	let input = clean_path(&std::path::absolute(folder).unwrap_or_else(|_| folder.to_path_buf()));
	if !input.is_dir() {
		return Err(Error::NotFolder(folder.to_path_buf()));
	}
	if format == Format::Gma && !has_addon_json(&input) {
		return Err(Error::NoAddonJson(folder.to_path_buf()));
	}

	let windows = launcher::is_windows_executable(&executable);
	let path = |host: &Path| launcher::tool_path(windows, &clean_path(host));

	let mut args = Vec::new();
	match format {
		Format::Vpk => {
			if options.multi_chunk {
				args.push("-M".to_owned());
			}
			args.extend(options.extra.iter().cloned());
			args.push(path(&input));
		}
		Format::Gma => {
			if let Some(parent) = output.parent() {
				std::fs::create_dir_all(parent).map_err(|e| Error::Io(parent.to_path_buf(), e))?;
			}
			args.extend([
				"create".to_owned(),
				"-folder".into(),
				path(&input),
				"-out".into(),
				path(output),
			]);
			if options.warn_invalid {
				args.push("-warninvalid".into());
			}
			args.extend(options.extra.iter().cloned());
		}
	}

	let mut command = launcher::build(app, &executable, args, Mode::Console, user_env)?;
	command.working_dir = input.parent().map(Path::to_path_buf);
	Ok(command)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackResult {
	/// The final package files; empty if the packer produced nothing.
	pub files: Vec<PathBuf>,
	pub log: Vec<String>,
}

/// `started` skips chunks left from an earlier, larger build.
pub fn finish_pack(
	format: Format,
	folder: &Path,
	output: &Path,
	options: &Options,
	started: SystemTime,
) -> PackResult {
	let mut result = PackResult::default();
	if format == Format::Gma {
		if output.exists() {
			result.files.push(output.to_path_buf());
		} else {
			result
				.log
				.push(t!("pack.not_created", path = output.display()).into_owned());
		}
		return result;
	}

	let input = clean_path(&std::path::absolute(folder).unwrap_or_else(|_| folder.to_path_buf()));
	let folder_name = input
		.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_default();
	let parent = input.parent().unwrap_or(Path::new("/"));
	let output_dir = output.parent().unwrap_or(Path::new("."));
	let output_stem = output
		.file_stem()
		.map(|s| s.to_string_lossy().into_owned())
		.unwrap_or_default();

	let move_to = |source: &Path, target: PathBuf, result: &mut PackResult| match move_file(
		source, &target,
	) {
		Ok(()) => {
			result.log.push(
				t!(
					"fs.moved",
					name = source.file_name().unwrap_or_default().to_string_lossy(),
					folder = output_dir.display()
				)
				.into_owned(),
			);
			result.files.push(target);
		}
		Err(_) => result.log.push(
			t!(
				"fs.move_failed",
				source = source.display(),
				target = target.display()
			)
			.into_owned(),
		),
	};

	if !options.multi_chunk {
		let packed = PathBuf::from(format!("{}.vpk", input.display()));
		if packed.exists() {
			move_to(&packed, output.to_path_buf(), &mut result);
		} else {
			result
				.log
				.push(t!("pack.not_created", path = packed.display()).into_owned());
		}
		return result;
	}

	let threshold = started
		.checked_sub(Duration::from_secs(2))
		.unwrap_or(started);
	let mut chunks: Vec<(PathBuf, String)> = std::fs::read_dir(parent)
		.map(|read| {
			read.flatten()
				.filter_map(|entry| {
					let name = entry.file_name().to_string_lossy().into_owned();
					let suffix = chunk_suffix(&name, &folder_name)?;
					let modified = entry.metadata().and_then(|m| m.modified()).ok()?;
					(modified >= threshold).then(|| (entry.path(), suffix))
				})
				.collect()
		})
		.unwrap_or_default();
	chunks.sort();
	for (source, suffix) in chunks {
		let target = output_dir.join(format!("{output_stem}_{suffix}.vpk"));
		move_to(&source, target, &mut result);
	}
	if result.files.is_empty() {
		result.log.push(
			t!(
				"pack.not_created",
				path = format!("{}_dir.vpk", input.display())
			)
			.into_owned(),
		);
	}
	result
}

/// "dir" or the chunk number of `<folder>_dir.vpk` / `<folder>_000.vpk`.
fn chunk_suffix(file_name: &str, folder_name: &str) -> Option<String> {
	let stem = file_name.get(..file_name.len().checked_sub(4)?)?;
	if !file_name[stem.len()..].eq_ignore_ascii_case(".vpk") {
		return None;
	}
	let (base, suffix) = stem.rsplit_once('_')?;
	let suffix = suffix.to_ascii_lowercase();
	let valid =
		suffix == "dir" || (suffix.len() == 3 && suffix.bytes().all(|b| b.is_ascii_digit()));
	(valid && base == folder_name).then_some(suffix)
}

/// Values gmad accepts in addon.json.
pub const ADDON_TYPES: [&str; 10] = [
	"gamemode",
	"map",
	"weapon",
	"vehicle",
	"npc",
	"entity",
	"tool",
	"effects",
	"model",
	"servercontent",
];
pub const ADDON_TAGS: [&str; 9] = [
	"fun", "roleplay", "scenic", "movie", "realism", "cartoon", "water", "comic", "build",
];

pub fn has_addon_json(folder: &Path) -> bool {
	folder.join("addon.json").exists()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddonInfo {
	pub title: String,
	pub kind: String,
	pub tags: Vec<String>,
}

/// The fields gmad cares about from `<folder>/addon.json`.
pub fn read_addon_json(folder: &Path) -> Result<AddonInfo> {
	let path = folder.join("addon.json");
	let bytes = std::fs::read(&path).map_err(|e| Error::Io(path.clone(), e))?;
	let root: serde_json::Value =
		serde_json::from_slice(&bytes).map_err(|e| Error::InvalidJson(e.to_string()))?;
	if !root.is_object() {
		return Err(Error::InvalidJson(String::new()));
	}
	let text = |key: &str| root[key].as_str().unwrap_or_default().to_owned();
	Ok(AddonInfo {
		title: text("title"),
		kind: text("type").to_lowercase(),
		tags: root["tags"]
			.as_array()
			.into_iter()
			.flatten()
			.map(|tag| tag.as_str().unwrap_or_default().to_lowercase())
			.collect(),
	})
}

/// Changes only title, type and tags; refuses a file it can't parse.
pub fn save_addon_json(folder: &Path, info: &AddonInfo) -> Result<()> {
	let path = folder.join("addon.json");
	let text = match std::fs::read(&path) {
		Ok(original) => {
			let parsed = serde_json::from_slice::<serde_json::Value>(&original)
				.map_err(|e| Error::InvalidJsonKept(e.to_string()))?;
			if !parsed.is_object() {
				return Err(Error::InvalidJsonKept(String::new()));
			}
			let original =
				String::from_utf8(original).map_err(|e| Error::InvalidJsonKept(e.to_string()))?;
			update_addon_json(&original, info).ok_or(Error::UnsafeUpdate)?
		}
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => new_addon_json(info),
		Err(e) => return Err(Error::Io(path, e)),
	};
	write_checked(&path, &text, info)
}

/// Writes a new addon.json; an existing one is the user's and is never replaced.
pub fn create_addon_json(folder: &Path, info: &AddonInfo) -> Result<()> {
	let path = folder.join("addon.json");
	if path.exists() {
		return Err(Error::AddonJsonExists);
	}
	write_checked(&path, &new_addon_json(info), info)
}

/// Never writes a file that would no longer parse or that lost the new values.
fn write_checked(path: &Path, text: &str, info: &AddonInfo) -> Result<()> {
	let check: serde_json::Value = serde_json::from_str(text).map_err(|_| Error::UnsafeUpdate)?;
	let tags: Vec<&str> = check["tags"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(serde_json::Value::as_str)
		.collect();
	if check["title"] != info.title.as_str()
		|| check["type"] != info.kind.as_str()
		|| tags != info.tags
	{
		return Err(Error::UnsafeUpdate);
	}
	let tmp = path.with_extension("json.tmp");
	std::fs::write(&tmp, text).map_err(|e| Error::Io(tmp.clone(), e))?;
	std::fs::rename(&tmp, path).map_err(|e| Error::Io(path.to_path_buf(), e))
}

fn json_string(value: &str) -> String {
	serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

fn new_addon_json(info: &AddonInfo) -> String {
	format!(
		"{{\n\t\"title\": {},\n\t\"type\": {},\n\t\"tags\": {},\n\t\"ignore\": [\"*.psd\", \"*.vcproj\", \"*.svn*\"]\n}}\n",
		json_string(&info.title),
		json_string(&info.kind),
		format_tags(&info.tags, "", "\t", "\n")
	)
}

/// Writes the tags the way the file already wrote its array.
fn format_tags(tags: &[String], original: &str, key_indent: &str, newline: &str) -> String {
	let items: Vec<String> = tags.iter().map(|tag| json_string(tag)).collect();
	if items.is_empty() {
		return "[]".into();
	}
	if original.contains('\n') {
		let mut item_indent = format!(
			"{key_indent}{}",
			if key_indent.starts_with(' ') {
				"    "
			} else {
				"\t"
			}
		);
		if let Some(first) = original.find('"').filter(|&i| i > 0) {
			let before = &original[..first];
			if let Some(newline_at) = before.rfind('\n')
				&& before[newline_at + 1..].trim().is_empty()
			{
				item_indent = before[newline_at + 1..].to_owned();
			}
		}
		let separator = format!(",{newline}{item_indent}");
		return format!(
			"[{newline}{item_indent}{}{newline}{key_indent}]",
			items.join(&separator)
		);
	}
	let pad = if original.starts_with("[ ") { " " } else { "" };
	let separator = if original.contains(", ") || !original.contains(',') {
		", "
	} else {
		","
	};
	format!("[{pad}{}{pad}]", items.join(separator))
}

/// A top-level member's byte span, for replacing in place.
struct Member {
	key: String,
	key_start: usize,
	key_end: usize,
	value_start: usize,
	value_end: usize,
}

struct ObjectLayout {
	open: usize,
	members: Vec<Member>,
}

fn skip_space(text: &str, mut pos: usize) -> usize {
	while let Some(c) = text[pos..].chars().next() {
		if !c.is_whitespace() && c != '\u{feff}' {
			break;
		}
		pos += c.len_utf8();
	}
	pos
}

/// `pos` is at the opening quote; returns the index just past the closing one.
fn skip_string(text: &str, pos: usize) -> Option<usize> {
	let bytes = text.as_bytes();
	let mut i = pos + 1;
	while i < bytes.len() {
		match bytes[i] {
			b'\\' => i += 2,
			b'"' => return Some(i + 1),
			_ => i += 1,
		}
	}
	None
}

fn skip_value(text: &str, pos: usize) -> Option<usize> {
	let bytes = text.as_bytes();
	match bytes.get(pos)? {
		b'"' => skip_string(text, pos),
		b'{' | b'[' => {
			let mut depth = 0;
			let mut i = pos;
			while i < bytes.len() {
				match bytes[i] {
					b'"' => {
						i = skip_string(text, i)?;
						continue;
					}
					b'{' | b'[' => depth += 1,
					b'}' | b']' => {
						depth -= 1;
						if depth == 0 {
							return Some(i + 1);
						}
					}
					_ => {}
				}
				i += 1;
			}
			None
		}
		_ => {
			let len = text[pos..]
				.find(|c: char| c.is_whitespace() || matches!(c, ',' | '}' | ']'))
				.unwrap_or(text.len() - pos);
			Some(pos + len)
		}
	}
}

fn scan_object(text: &str) -> Option<ObjectLayout> {
	let bytes = text.as_bytes();
	let mut pos = skip_space(text, 0);
	if bytes.get(pos) != Some(&b'{') {
		return None;
	}
	let mut layout = ObjectLayout {
		open: pos,
		members: Vec::new(),
	};
	pos += 1;
	loop {
		pos = skip_space(text, pos);
		match bytes.get(pos)? {
			b'}' => return Some(layout),
			b'"' => {}
			_ => return None,
		}
		let key_start = pos;
		let key_end = skip_string(text, pos)?;
		let key = text[key_start + 1..key_end - 1].to_owned();
		pos = skip_space(text, key_end);
		if bytes.get(pos) != Some(&b':') {
			return None;
		}
		let value_start = skip_space(text, pos + 1);
		let value_end = skip_value(text, value_start)?;
		layout.members.push(Member {
			key,
			key_start,
			key_end,
			value_start,
			value_end,
		});
		pos = skip_space(text, value_end);
		if bytes.get(pos) == Some(&b',') {
			pos += 1;
		}
	}
}

/// The whitespace before `pos` on its line, if nothing else precedes it there.
fn line_indent(text: &str, pos: usize) -> &str {
	let start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
	let prefix = &text[start..pos];
	if prefix.trim().is_empty() { prefix } else { "" }
}

/// Everything but title, type and tags stays byte for byte.
fn update_addon_json(text: &str, info: &AddonInfo) -> Option<String> {
	let layout = scan_object(text)?;
	let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
	let (indent, separator) = match layout.members.first() {
		Some(first) => (
			line_indent(text, first.key_start),
			&text[first.key_end..first.value_start],
		),
		None => ("\t", ": "),
	};

	let format = |key: &str, original: &str, key_indent: &str| match key {
		"title" => json_string(&info.title),
		"type" => json_string(&info.kind),
		_ => format_tags(&info.tags, original, key_indent, newline),
	};

	let mut edits: Vec<(usize, usize, String)> = Vec::new();
	let mut inserts = Vec::new();
	for key in ["title", "type", "tags"] {
		let mut found = false;
		for member in layout.members.iter().filter(|m| m.key == key) {
			found = true;
			let original = &text[member.value_start..member.value_end];
			edits.push((
				member.value_start,
				member.value_end,
				format(key, original, line_indent(text, member.key_start)),
			));
		}
		if !found {
			inserts.push(format!(
				"{}{separator}{}",
				json_string(key),
				format(key, "", indent)
			));
		}
	}
	if !inserts.is_empty() {
		let joined = inserts.join(&format!(",{newline}{indent}"));
		match layout.members.last() {
			None => {
				let at = layout.open + 1;
				edits.push((at, at, format!("{newline}{indent}{joined}{newline}")));
			}
			Some(last) => {
				let at = last.value_end;
				edits.push((at, at, format!(",{newline}{indent}{joined}")));
			}
		}
	}

	edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
	let mut result = text.to_owned();
	for (start, end, replacement) in edits {
		result.replace_range(start..end, &replacement);
	}
	Some(result)
}

#[cfg(test)]
mod tests {
	use super::*;
	use launcher::to_windows_path;

	fn write(path: &Path, content: &[u8]) {
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		std::fs::write(path, content).unwrap();
	}

	fn read(path: &Path) -> String {
		std::fs::read_to_string(path).unwrap_or_else(|_| "<missing>".into())
	}

	fn info(title: &str, kind: &str, tags: &[&str]) -> AddonInfo {
		AddonInfo {
			title: title.into(),
			kind: kind.into(),
			tags: tags.iter().map(|t| (*t).to_owned()).collect(),
		}
	}

	fn multi_chunk() -> Options {
		Options {
			multi_chunk: true,
			..Options::default()
		}
	}

	#[test]
	fn pack_commands() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path();
		write(&dir.join("bin/vpk.exe"), b"MZ");
		write(&dir.join("bin/gmad.exe"), b"MZ");
		write(&dir.join("addon/lua/a.lua"), b"x");
		let app = App {
			id: 4000,
			tools_path: Some(dir.join("bin")),
			proton_path: Some(dir.join("proton")),
			compat_data_path: Some(dir.join("compatdata")),
			..App::default()
		};
		let addon = dir.join("addon");
		let win_addon = to_windows_path(&addon);
		let out = dir.join("out/a.gma");

		let command = pack_command(
			&app,
			Format::Vpk,
			&addon,
			Path::new(""),
			&Options::default(),
			"",
		)
		.unwrap();
		assert_eq!(
			command.args,
			[
				"runinprefix".to_owned(),
				dir.join("bin/vpk.exe").display().to_string(),
				win_addon.clone()
			]
		);
		let command = pack_command(
			&app,
			Format::Vpk,
			&dir.join("addon/"),
			Path::new(""),
			&multi_chunk(),
			"",
		)
		.unwrap();
		assert_eq!(command.args[2..], ["-M".to_owned(), win_addon.clone()]);

		assert!(matches!(
			pack_command(&app, Format::Gma, &addon, &out, &Options::default(), ""),
			Err(Error::NoAddonJson(_))
		));

		write(&addon.join("addon.json"), b"{}");
		let command =
			pack_command(&app, Format::Gma, &addon, &out, &Options::default(), "").unwrap();
		assert_eq!(
			command.args[2..],
			[
				"create".to_owned(),
				"-folder".into(),
				win_addon.clone(),
				"-out".into(),
				to_windows_path(&out)
			]
		);

		let mut flags = Options {
			warn_invalid: true,
			extra: vec!["-quiet".into()],
			..Options::default()
		};
		let command = pack_command(&app, Format::Gma, &addon, &out, &flags, "").unwrap();
		assert_eq!(command.args[7..], ["-warninvalid", "-quiet"]);
		flags.multi_chunk = true;
		flags.extra = vec!["-c".into(), "100".into()];
		let command = pack_command(&app, Format::Vpk, &addon, Path::new(""), &flags, "").unwrap();
		assert_eq!(
			command.args[2..],
			["-M".to_owned(), "-c".into(), "100".into(), win_addon]
		);
	}

	#[test]
	fn finish() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path();
		let addon = dir.join("addon");
		std::fs::create_dir_all(&addon).unwrap();
		let started = SystemTime::now();
		let single = dir.join("elsewhere/final.vpk");

		assert!(
			finish_pack(Format::Vpk, &addon, &single, &Options::default(), started)
				.files
				.is_empty()
		);
		write(&dir.join("addon.vpk"), b"packed");
		assert_eq!(
			finish_pack(Format::Vpk, &addon, &single, &Options::default(), started).files,
			std::slice::from_ref(&single)
		);
		assert_eq!(read(&single), "packed");
		assert!(!dir.join("addon.vpk").exists());

		write(&dir.join("addon_dir.vpk"), b"d");
		write(&dir.join("addon_000.vpk"), b"0");
		write(&dir.join("addon_001.vpk"), b"1");
		write(&dir.join("addon_extra_dir.vpk"), b"other");
		let multi = dir.join("multi/pak01.vpk");
		assert_eq!(
			finish_pack(Format::Vpk, &addon, &multi, &multi_chunk(), started).files,
			[
				dir.join("multi/pak01_000.vpk"),
				dir.join("multi/pak01_001.vpk"),
				dir.join("multi/pak01_dir.vpk")
			]
		);
		assert!(dir.join("addon_extra_dir.vpk").exists());

		// Chunks older than this run belong to a previous build.
		write(&dir.join("addon_dir.vpk"), b"stale");
		let later = SystemTime::now() + Duration::from_secs(60);
		assert!(
			finish_pack(
				Format::Vpk,
				&addon,
				&dir.join("multi2/a.vpk"),
				&multi_chunk(),
				later
			)
			.files
			.is_empty()
		);
	}

	#[test]
	fn addon_json_read_and_save() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path();
		assert!(read_addon_json(dir).is_err());

		write(
			&dir.join("addon.json"),
			br#"{"title": "Old", "type": "Weapon", "tags": ["Fun"], "ignore": ["*.txt"], "description": "keep me"}"#,
		);
		assert_eq!(
			read_addon_json(dir).unwrap(),
			info("Old", "weapon", &["fun"])
		);

		save_addon_json(dir, &info("New", "tool", &["build", "fun"])).unwrap();
		let saved: serde_json::Value =
			serde_json::from_str(&read(&dir.join("addon.json"))).unwrap();
		assert_eq!(saved["title"], "New");
		assert_eq!(saved["type"], "tool");
		assert_eq!(saved["tags"].as_array().unwrap().len(), 2);
		assert_eq!(saved["description"], "keep me");
		assert_eq!(saved["ignore"][0], "*.txt");
		assert_eq!(
			read_addon_json(dir).unwrap(),
			info("New", "tool", &["build", "fun"])
		);

		write(&dir.join("addon.json"), b"{ broken, }");
		assert!(matches!(read_addon_json(dir), Err(Error::InvalidJson(_))));
		assert!(matches!(
			save_addon_json(dir, &info("X", "tool", &[])),
			Err(Error::InvalidJsonKept(_))
		));
		assert_eq!(read(&dir.join("addon.json")), "{ broken, }");

		let fresh = dir.join("fresh");
		std::fs::create_dir_all(&fresh).unwrap();
		save_addon_json(&fresh, &info("Fresh", "model", &[])).unwrap();
		let fresh_json: serde_json::Value =
			serde_json::from_str(&read(&fresh.join("addon.json"))).unwrap();
		assert!(fresh_json["ignore"].is_array());
	}

	#[test]
	fn addon_json_keeps_formatting() {
		let cases = [
			(
				"{\n\t\"title\": \"Old\",\n\t\"type\": \"weapon\",\n\t\"tags\": [\"fun\", \"realism\"],\n\t\"ignore\": [\"*.psd\"],\n\t\"description\": \"keep\"\n}\n",
				"{\n\t\"title\": \"New \\\"one\\\"\",\n\t\"type\": \"tool\",\n\t\"tags\": [\"build\"],\n\t\"ignore\": [\"*.psd\"],\n\t\"description\": \"keep\"\n}\n",
			),
			(
				"{\r\n  \"ignore\": [],\r\n  \"tags\": [\r\n    \"fun\"\r\n  ],\r\n  \"type\": \"weapon\",\r\n  \"title\": \"Old\"\r\n}",
				"{\r\n  \"ignore\": [],\r\n  \"tags\": [\r\n    \"build\"\r\n  ],\r\n  \"type\": \"tool\",\r\n  \"title\": \"New \\\"one\\\"\"\r\n}",
			),
			(
				"{\n\t\"title\": \"Old\",\n\t\"ignore\": [\"*.psd\"]\n}\n",
				"{\n\t\"title\": \"New \\\"one\\\"\",\n\t\"ignore\": [\"*.psd\"],\n\t\"type\": \"tool\",\n\t\"tags\": [\"build\"]\n}\n",
			),
			(
				"{}",
				"{\n\t\"title\": \"New \\\"one\\\"\",\n\t\"type\": \"tool\",\n\t\"tags\": [\"build\"]\n}",
			),
			(
				"{\"title\":\"Тест\",\"x\":{\"title\":\"inner\"},\"tags\":[\"fun\",\"water\"],\"type\":\"map\"}",
				"{\"title\":\"New \\\"one\\\"\",\"x\":{\"title\":\"inner\"},\"tags\":[\"build\"],\"type\":\"tool\"}",
			),
		];
		for (before, after) in cases {
			let temp = tempfile::tempdir().unwrap();
			write(&temp.path().join("addon.json"), before.as_bytes());
			save_addon_json(temp.path(), &info("New \"one\"", "tool", &["build"])).unwrap();
			assert_eq!(read(&temp.path().join("addon.json")), after, "{before}");
		}
	}

	#[test]
	fn new_addon_json_uses_tabs() {
		let temp = tempfile::tempdir().unwrap();
		save_addon_json(temp.path(), &info("A", "model", &["fun", "build"])).unwrap();
		assert_eq!(
			read(&temp.path().join("addon.json")),
			"{\n\t\"title\": \"A\",\n\t\"type\": \"model\",\n\t\"tags\": [\"fun\", \"build\"],\n\t\"ignore\": [\"*.psd\", \"*.vcproj\", \"*.svn*\"]\n}\n"
		);
	}

	#[test]
	fn create_never_replaces() {
		let temp = tempfile::tempdir().unwrap();
		assert!(!has_addon_json(temp.path()));
		create_addon_json(
			temp.path(),
			&info("My \"Addon\"", "model", &["fun", "build"]),
		)
		.unwrap();
		assert!(has_addon_json(temp.path()));
		assert_eq!(
			read_addon_json(temp.path()).unwrap(),
			info("My \"Addon\"", "model", &["fun", "build"])
		);
		assert!(matches!(
			create_addon_json(temp.path(), &info("Other", "map", &[])),
			Err(Error::AddonJsonExists)
		));
		assert_eq!(read_addon_json(temp.path()).unwrap().kind, "model");
		assert!(ADDON_TYPES.contains(&"servercontent"));
	}
}
