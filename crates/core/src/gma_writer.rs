//! GMA v3 writer, byte for byte like gmad.

use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use rust_i18n::t;

use crate::gma_whitelist;
use crate::packer::{ADDON_TAGS, ADDON_TYPES};

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("{}", t!("gma.bad_json", error = .0))]
	BadJson(String),
	#[error("{}", t!("gma.no_title"))]
	NoTitle,
	#[error("{}", t!("gma.no_type"))]
	NoType,
	#[error("{}", t!("gma.bad_type", kind = .0))]
	BadType(String),
	#[error("{}", t!("gma.bad_tag", tag = .0))]
	BadTag(String),
	#[error("{}", t!("gma.too_many_tags"))]
	TooManyTags,
	#[error("{}", t!("gma.no_files"))]
	NoFiles,
	#[error("{}", t!("gma.invalid_files", n = .0.len(), first = .0[0]))]
	InvalidFiles(Vec<String>),
	#[error("{}", t!("gma.io", path = .0.display(), error = .1))]
	Io(PathBuf, #[source] io::Error),
	#[error("{}", t!("gma.cancelled"))]
	Cancelled,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What addon.json says, checked the way gmad checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addon {
	pub title: String,
	pub description: String,
	pub kind: String,
	pub tags: Vec<String>,
}

pub fn read_addon(folder: &Path) -> Result<Addon> {
	let path = folder.join("addon.json");
	let bytes = std::fs::read(&path).map_err(|e| Error::Io(path, e))?;
	let root: serde_json::Value =
		serde_json::from_slice(&bytes).map_err(|e| Error::BadJson(e.to_string()))?;
	let text = |key: &str| root[key].as_str().unwrap_or_default().to_owned();
	let title = text("title");
	if title.is_empty() {
		return Err(Error::NoTitle);
	}
	let kind = text("type").to_lowercase();
	if kind.is_empty() {
		return Err(Error::NoType);
	}
	if !ADDON_TYPES.contains(&kind.as_str()) {
		return Err(Error::BadType(kind));
	}
	let tags: Vec<String> = root["tags"]
		.as_array()
		.into_iter()
		.flatten()
		.map(|tag| tag.as_str().unwrap_or_default().to_lowercase())
		.collect();
	if tags.len() > 2 {
		return Err(Error::TooManyTags);
	}
	if let Some(bad) = tags.iter().find(|t| !ADDON_TAGS.contains(&t.as_str())) {
		return Err(Error::BadTag(bad.clone()));
	}
	let description = match root["description"].as_str() {
		Some(text) if !text.is_empty() => text.to_owned(),
		// gmad's own default.
		_ => "Description".to_owned(),
	};
	Ok(Addon {
		title,
		description,
		kind,
		tags,
	})
}

/// The JSON gmad stores as the description, byte for byte.
fn description_json(addon: &Addon) -> String {
	let quote = |text: &str| serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
	let tags = if addon.tags.is_empty() {
		"[]".to_owned()
	} else {
		let items: Vec<String> = addon
			.tags
			.iter()
			.map(|tag| format!("\t\t{}", quote(tag)))
			.collect();
		format!("[\n{}\n\t]", items.join(",\n"))
	};
	format!(
		"{{\n\t\"description\": {},\n\t\"type\": {},\n\t\"tags\": {}\n}}",
		quote(&addon.description),
		quote(&addon.kind),
		tags
	)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Packed {
	/// Paths as stored: lowercase.
	pub files: Vec<String>,
	/// Not on the whitelist, left out.
	pub skipped: Vec<String>,
	/// Same path as another file but for case; gmad keeps one of them too.
	pub duplicates: Vec<String>,
	pub size: u64,
}

struct Entry {
	source: PathBuf,
	name: String,
	size: u64,
	crc: u32,
}

/// Through a temp file; `on_file(name, done, total)` runs before each file.
pub fn pack(
	folder: &Path,
	output: &Path,
	skip_invalid: bool,
	mut on_file: impl FnMut(&str, usize, usize),
	cancelled: impl Fn() -> bool,
) -> Result<Packed> {
	let addon = read_addon(folder)?;
	let check = gma_whitelist::check(folder).map_err(|e| Error::Io(folder.to_path_buf(), e))?;
	if !check.invalid.is_empty() && !skip_invalid {
		return Err(Error::InvalidFiles(check.invalid));
	}

	let mut names: Vec<(String, String)> = check
		.files
		.into_iter()
		.map(|file| (file.to_lowercase(), file))
		.collect();
	names.sort();
	let mut packed = Packed {
		skipped: check.invalid,
		..Packed::default()
	};
	let mut entries: Vec<Entry> = Vec::with_capacity(names.len());
	let total = names.len();
	for (done, (name, original)) in names.into_iter().enumerate() {
		if entries.last().is_some_and(|e| e.name == name) {
			packed.duplicates.push(original);
			continue;
		}
		if cancelled() {
			return Err(Error::Cancelled);
		}
		on_file(&name, done, total);
		let source = folder.join(&original);
		let (size, crc) = crc_of(&source).map_err(|e| Error::Io(source.clone(), e))?;
		entries.push(Entry {
			source,
			name,
			size,
			crc,
		});
	}
	if entries.is_empty() {
		return Err(Error::NoFiles);
	}

	let dir = output.parent().unwrap_or(Path::new("."));
	std::fs::create_dir_all(dir).map_err(|e| Error::Io(dir.to_path_buf(), e))?;
	let temp = tempfile::Builder::new()
		.prefix(".prybar-gma")
		.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o644))
		.tempfile_in(dir)
		.map_err(|e| Error::Io(dir.to_path_buf(), e))?;
	let io_err = |e| Error::Io(output.to_path_buf(), e);
	let mut out = Crc32Writer::new(BufWriter::new(temp.as_file()));

	out.write_all(b"GMAD\x03").map_err(io_err)?;
	out.write_all(&0u64.to_le_bytes()).map_err(io_err)?;
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |d| d.as_secs());
	out.write_all(&now.to_le_bytes()).map_err(io_err)?;
	out.write_all(b"\0").map_err(io_err)?;
	for text in [
		addon.title.as_str(),
		&description_json(&addon),
		"Author Name",
	] {
		out.write_all(text.as_bytes()).map_err(io_err)?;
		out.write_all(b"\0").map_err(io_err)?;
	}
	out.write_all(&1i32.to_le_bytes()).map_err(io_err)?;
	for (number, entry) in (1u32..).zip(&entries) {
		out.write_all(&number.to_le_bytes()).map_err(io_err)?;
		out.write_all(entry.name.as_bytes()).map_err(io_err)?;
		out.write_all(b"\0").map_err(io_err)?;
		out.write_all(&(entry.size as i64).to_le_bytes())
			.map_err(io_err)?;
		out.write_all(&entry.crc.to_le_bytes()).map_err(io_err)?;
	}
	out.write_all(&0u32.to_le_bytes()).map_err(io_err)?;

	for entry in &entries {
		if cancelled() {
			return Err(Error::Cancelled);
		}
		let file =
			std::fs::File::open(&entry.source).map_err(|e| Error::Io(entry.source.clone(), e))?;
		// A file that changed since its CRC was taken would make a broken addon.
		let copied = io::copy(&mut file.take(entry.size + 1), &mut out)
			.map_err(|e| Error::Io(entry.source.clone(), e))?;
		if copied != entry.size {
			return Err(Error::Io(
				entry.source.clone(),
				io::Error::other(t!("gma.changed").into_owned()),
			));
		}
	}
	let crc = out.crc();
	let mut inner = out.into_inner();
	inner.write_all(&crc.to_le_bytes()).map_err(io_err)?;
	inner.flush().map_err(io_err)?;
	drop(inner);
	temp.persist(output)
		.map_err(|e| Error::Io(output.to_path_buf(), e.error))?;

	packed.size = std::fs::metadata(output).map_or(0, |m| m.len());
	packed.files = entries.into_iter().map(|e| e.name).collect();
	Ok(packed)
}

fn crc_of(path: &Path) -> io::Result<(u64, u32)> {
	let mut file = std::fs::File::open(path)?;
	let mut hasher = crc32fast::Hasher::new();
	let mut buf = vec![0; 1 << 16];
	let mut size = 0;
	loop {
		let n = file.read(&mut buf)?;
		if n == 0 {
			break;
		}
		hasher.update(&buf[..n]);
		size += n as u64;
	}
	Ok((size, hasher.finalize()))
}

struct Crc32Writer<W> {
	inner: W,
	hasher: crc32fast::Hasher,
}

impl<W: Write> Crc32Writer<W> {
	fn new(inner: W) -> Self {
		Self {
			inner,
			hasher: crc32fast::Hasher::new(),
		}
	}

	fn crc(&self) -> u32 {
		self.hasher.clone().finalize()
	}

	fn into_inner(self) -> W {
		self.inner
	}
}

impl<W: Write> Write for Crc32Writer<W> {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		let n = self.inner.write(buf)?;
		self.hasher.update(&buf[..n]);
		Ok(n)
	}

	fn flush(&mut self) -> io::Result<()> {
		self.inner.flush()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn addon(dir: &Path, json: &str, files: &[(&str, &str)]) {
		std::fs::write(dir.join("addon.json"), json).unwrap();
		for (path, data) in files {
			let path = dir.join(path);
			std::fs::create_dir_all(path.parent().unwrap()).unwrap();
			std::fs::write(path, data).unwrap();
		}
	}

	/// Bytes gmad 1.1 wrote for the same folder, but for the timestamp.
	#[test]
	fn matches_gmad() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path().join("a");
		std::fs::create_dir(&dir).unwrap();
		addon(
			&dir,
			r#"{"title":"two","type":"weapon","tags":["fun","Build"]}"#,
			&[("lua/x.lua", "y")],
		);
		let out = temp.path().join("a.gma");
		pack(&dir, &out, false, |_, _, _| {}, || false).unwrap();
		let ours = std::fs::read(&out).unwrap();

		let mut gmad = b"GMAD\x03\0\0\0\0\0\0\0\0".to_vec();
		gmad.extend_from_slice(&ours[13..21]);
		gmad.extend_from_slice(b"\0two\0{\n\t\"description\": \"Description\",\n\t\"type\": \"weapon\",\n\t\"tags\": [\n\t\t\"fun\",\n\t\t\"build\"\n\t]\n}\0Author Name\0\x01\0\0\0");
		gmad.extend_from_slice(b"\x01\0\0\0lua/x.lua\0\x01\0\0\0\0\0\0\0\x15\x26\xdb\xfb\0\0\0\0y");
		let crc = crc32fast::hash(&gmad);
		gmad.extend_from_slice(&crc.to_le_bytes());
		assert_eq!(ours, gmad);
	}

	#[test]
	fn reads_back_and_sorts() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path().join("a");
		std::fs::create_dir(&dir).unwrap();
		addon(
			&dir,
			r#"{"title":"T","type":"Model","tags":[],"description":"line1\nline2 \"x\""}"#,
			&[
				("lua/b.lua", "bb"),
				("lua/A.lua", "a1"),
				("lua/a.lua", "a2"),
				("lua/a/z.lua", "z"),
				("readme.txt", "no"),
			],
		);
		let out = temp.path().join("a.gma");
		assert!(matches!(
			pack(&dir, &out, false, |_, _, _| {}, || false),
			Err(Error::InvalidFiles(_))
		));
		let packed = pack(&dir, &out, true, |_, _, _| {}, || false).unwrap();
		assert_eq!(packed.files, ["lua/a.lua", "lua/a/z.lua", "lua/b.lua"]);
		assert_eq!(packed.skipped, ["readme.txt"]);
		assert_eq!(packed.duplicates.len(), 1);

		let package = crate::package::Package::open(&out).unwrap();
		let names: Vec<_> = package.entries.iter().map(|e| e.path.as_str()).collect();
		assert_eq!(names, ["lua/a.lua", "lua/a/z.lua", "lua/b.lua"]);
		let bytes = std::fs::read(&out).unwrap();
		let text = String::from_utf8_lossy(&bytes);
		assert!(text.contains(r#""description": "line1\nline2 \"x\"","#));
		assert!(text.contains("\"tags\": []\n}"));
	}

	#[test]
	fn checks_addon_json() {
		let temp = tempfile::tempdir().unwrap();
		for (json, expected) in [
			(r#"{"type":"model"}"#, "NoTitle"),
			(r#"{"title":"x"}"#, "NoType"),
			(r#"{"title":"x","type":"nope"}"#, "BadType"),
			(r#"{"title":"x","type":"map","tags":["nope"]}"#, "BadTag"),
			(
				r#"{"title":"x","type":"map","tags":["fun","build","comic"]}"#,
				"TooManyTags",
			),
			(r#"{"title":"x","#, "BadJson"),
		] {
			std::fs::write(temp.path().join("addon.json"), json).unwrap();
			let error = format!("{:?}", read_addon(temp.path()).unwrap_err());
			assert!(error.starts_with(expected), "{json}: {error}");
		}
	}
}
