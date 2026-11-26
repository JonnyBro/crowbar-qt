//! Which files a QC is built from, and noticing when they change.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What a QC can pull in; a bare name is also tried with the first two.
const SOURCE_EXTENSIONS: [&str; 8] = ["smd", "dmx", "vta", "qci", "qc", "vrd", "fbx", "obj"];

/// Resolved like studiomdl: through `$pushd`, in any letter case.
pub fn dependencies(qc: &Path) -> Vec<PathBuf> {
	let mut found = Vec::new();
	let mut seen = HashSet::new();
	let base = qc.parent().map(Path::to_path_buf).unwrap_or_default();
	let mut dirs = vec![base];
	let mut listings = Listings::default();
	collect(qc, &mut dirs, &mut seen, &mut found, &mut listings);
	found
}

/// Folder listings by lowercase name, read once per look.
#[derive(Default)]
struct Listings(HashMap<PathBuf, HashMap<String, PathBuf>>);

impl Listings {
	fn find(&mut self, base: &Path, relative: &str) -> Option<PathBuf> {
		let mut current = base.to_path_buf();
		for part in relative.split('/').filter(|p| !p.is_empty() && *p != ".") {
			if part == ".." {
				current.pop();
				continue;
			}
			let listing = self.0.entry(current.clone()).or_insert_with(|| {
				std::fs::read_dir(&current)
					.map(|read| {
						read.flatten()
							.map(|e| (e.file_name().to_string_lossy().to_lowercase(), e.path()))
							.collect()
					})
					.unwrap_or_default()
			});
			current = listing.get(&part.to_lowercase())?.clone();
		}
		Some(current)
	}
}

fn collect(
	file: &Path,
	dirs: &mut Vec<PathBuf>,
	seen: &mut HashSet<PathBuf>,
	found: &mut Vec<PathBuf>,
	listings: &mut Listings,
) {
	let key = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
	if seen.len() > 512 || !seen.insert(key) {
		return;
	}
	found.push(file.to_path_buf());
	let Ok(bytes) = std::fs::read(file) else {
		return;
	};
	let text = String::from_utf8_lossy(&bytes);
	let tokens = tokens(&text);
	let mut i = 0;
	while i < tokens.len() {
		let token = tokens[i];
		let dir = dirs.last().cloned().unwrap_or_default();
		if token.eq_ignore_ascii_case("$pushd") {
			if let Some(next) = tokens.get(i + 1) {
				dirs.push(dir.join(next.replace('\\', "/")));
				i += 1;
			}
		} else if token.eq_ignore_ascii_case("$popd") {
			if dirs.len() > 1 {
				dirs.pop();
			}
		} else if let Some(path) = resolve(&dir, file, token, listings) {
			let extension = path
				.extension()
				.map(|e| e.to_string_lossy().to_lowercase())
				.unwrap_or_default();
			if matches!(extension.as_str(), "qc" | "qci") {
				collect(&path, dirs, seen, found, listings);
			} else if seen.insert(path.canonicalize().unwrap_or_else(|_| path.clone())) {
				found.push(path);
			}
		}
		i += 1;
	}
}

/// Looked up in the current folder, then in the folder of the file that names it.
fn resolve(dir: &Path, file: &Path, token: &str, listings: &mut Listings) -> Option<PathBuf> {
	if token.starts_with('$') || token.len() < 2 {
		return None;
	}
	let token = token.replace('\\', "/");
	let lower = token.to_lowercase();
	let has_extension = SOURCE_EXTENSIONS
		.iter()
		.any(|e| lower.ends_with(&format!(".{e}")));
	let names: Vec<String> = if has_extension {
		vec![token]
	} else {
		SOURCE_EXTENSIONS[..2]
			.iter()
			.map(|e| format!("{token}.{e}"))
			.collect()
	};
	let file_dir = file.parent().unwrap_or(dir);
	names.iter().find_map(|name| {
		[dir, file_dir]
			.into_iter()
			.find_map(|base| listings.find(base, name))
			.filter(|p| p.is_file())
	})
}

/// Words and quoted strings, without comments or braces.
fn tokens(text: &str) -> Vec<&str> {
	let mut out = Vec::new();
	let bytes = text.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		match bytes[i] {
			b'/' if bytes.get(i + 1) == Some(&b'/') => {
				while i < bytes.len() && bytes[i] != b'\n' {
					i += 1;
				}
			}
			b'/' if bytes.get(i + 1) == Some(&b'*') => {
				i = text[i + 2..]
					.find("*/")
					.map_or(bytes.len(), |end| i + 2 + end + 2);
			}
			b'"' => {
				let end = text[i + 1..]
					.find(['"', '\n'])
					.map_or(bytes.len(), |end| i + 1 + end);
				out.push(&text[i + 1..end]);
				i = end + 1;
			}
			b if b.is_ascii_whitespace() || b == b'{' || b == b'}' => i += 1,
			_ => {
				let end = text[i..]
					.find(|c: char| c.is_whitespace() || matches!(c, '"' | '{' | '}'))
					.map_or(bytes.len(), |end| i + end);
				out.push(&text[i..end]);
				i = end;
			}
		}
	}
	out
}

type Stamps = Vec<(PathBuf, Option<(SystemTime, u64)>)>;

fn stamps(files: Vec<PathBuf>) -> Stamps {
	files
		.into_iter()
		.map(|f| {
			let stamp = std::fs::metadata(&f)
				.ok()
				.map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
			(f, stamp)
		})
		.collect()
}

/// Remembers each QC's files as they were when it was last built.
#[derive(Debug, Default)]
pub struct Watcher {
	built: HashMap<PathBuf, Stamps>,
	/// A change counts once the next look finds it the same, so a half-saved file waits.
	pending: HashMap<PathBuf, Stamps>,
}

impl Watcher {
	/// Marks the files of `qc` as built in their current state.
	pub fn built(&mut self, qc: &Path) {
		self.pending.remove(qc);
		self.built
			.insert(qc.to_path_buf(), stamps(dependencies(qc)));
	}

	/// A QC seen for the first time is only remembered.
	pub fn poll(&mut self, qcs: &[PathBuf]) -> Vec<PathBuf> {
		self.built.retain(|qc, _| qcs.contains(qc));
		self.pending.retain(|qc, _| qcs.contains(qc));
		let mut changed = Vec::new();
		for qc in qcs {
			let Some(built) = self.built.get(qc) else {
				self.built(qc);
				continue;
			};
			let now = stamps(dependencies(qc));
			if &now == built {
				self.pending.remove(qc);
			} else if self.pending.get(qc) == Some(&now) {
				changed.push(qc.clone());
			} else {
				self.pending.insert(qc.clone(), now);
			}
		}
		changed
	}
}

#[cfg(test)]
mod tests;
