//! VPK v2 writer matching Valve's vpk, found by testing it:
//! - paths are lowercase; no extension or the root folder is " ";
//! - data is sorted by upper-cased path components;
//! - the tree is in reverse order of first appearance;
//! - dot files and [`REFUSED`] types are skipped;
//! - with -M a chunk closes at the chunk size, empty files stay in the dir file;
//! - it ends with MD5s of the tree, the empty chunk MD5s and all before.

use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use rust_i18n::t;

const SIGNATURE: u32 = 0x55aa_1234;
/// Archive index of data stored in the directory file itself.
const IN_DIR: u16 = 0x7fff;
/// "Ignoring %s: unsupported file type." in vpk, compared without case.
const REFUSED: &[&str] = &[
	"bat", "cmd", "com", "dll", "exe", "msi", "rar", "reg", "zip",
];
/// vpk's default for -c, in megabytes.
pub const DEFAULT_CHUNK_MB: u64 = 200;

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("{}", t!("vpk.no_files"))]
	NoFiles,
	#[error("{}", t!("vpk.io", path = .0.display(), error = .1))]
	Io(PathBuf, #[source] io::Error),
	#[error("{}", t!("vpk.cancelled"))]
	Cancelled,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
	/// Data in numbered `<name>_000.vpk` chunks next to `<name>_dir.vpk`.
	pub multi_chunk: bool,
	pub chunk_size: u64,
}

impl Default for Options {
	fn default() -> Self {
		Self {
			multi_chunk: false,
			chunk_size: DEFAULT_CHUNK_MB * 1024 * 1024,
		}
	}
}

impl Options {
	/// Takes vpk's `-c <MB>` from `args`; any other vpk option needs vpk itself.
	pub fn from_args(multi_chunk: bool, args: &[String]) -> Result<Self, String> {
		let mut options = Self {
			multi_chunk,
			..Self::default()
		};
		let mut args = args.iter();
		while let Some(arg) = args.next() {
			match arg.as_str() {
				"-c" => {
					let megabytes = args
						.next()
						.and_then(|n| n.parse::<u64>().ok())
						.filter(|&n| n > 0)
						.ok_or_else(|| t!("vpk.bad_chunk_size").into_owned())?;
					options.chunk_size = megabytes * 1024 * 1024;
				}
				other => return Err(t!("vpk.unsupported_option", option = other).into_owned()),
			}
		}
		Ok(options)
	}
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Packed {
	/// The VPK files written: one, or the directory file and its chunks.
	pub outputs: Vec<PathBuf>,
	pub files: usize,
	pub data_size: u64,
	/// Paths vpk would leave out, relative to the folder.
	pub skipped: Vec<String>,
}

struct Entry {
	source: PathBuf,
	ext: String,
	dir: String,
	name: String,
	size: u64,
	crc: u32,
	archive: u16,
	offset: u32,
}

/// Splits a lowercase relative path the way the tree stores it.
fn split(path: &str) -> (String, String, String) {
	let (dir, file) = match path.rsplit_once('/') {
		Some((dir, file)) => (dir.to_owned(), file),
		None => (" ".to_owned(), path),
	};
	let (name, ext) = match file.rsplit_once('.') {
		Some((name, ext)) if !ext.is_empty() => (name.to_owned(), ext.to_owned()),
		_ => (file.to_owned(), " ".to_owned()),
	};
	(ext, dir, name)
}

/// vpk compares names upper-cased, so "_" sorts after the letters.
fn sort_key(path: &str) -> Vec<String> {
	path.to_ascii_uppercase()
		.split('/')
		.map(str::to_owned)
		.collect()
}

fn collect(
	dir: &Path,
	prefix: &str,
	files: &mut Vec<String>,
	skipped: &mut Vec<String>,
) -> io::Result<()> {
	for entry in std::fs::read_dir(dir)? {
		let entry = entry?;
		let name = entry.file_name().to_string_lossy().into_owned();
		let hidden = name.starts_with('.');
		let relative = if prefix.is_empty() {
			name
		} else {
			format!("{prefix}/{name}")
		};
		let path = entry.path();
		if path.is_dir() {
			if hidden {
				skipped.push(format!("{relative}/"));
			} else {
				collect(&path, &relative, files, skipped)?;
			}
		} else if path.is_file() {
			let refused = Path::new(&relative)
				.extension()
				.and_then(|e| e.to_str())
				.is_some_and(|e| REFUSED.iter().any(|r| e.eq_ignore_ascii_case(r)));
			if hidden || refused {
				skipped.push(relative);
			} else {
				files.push(relative);
			}
		}
	}
	Ok(())
}

/// `output` is the VPK, or `<name>.vpk` for a chunked one; written through temp files.
pub fn pack(
	folder: &Path,
	output: &Path,
	options: Options,
	mut on_file: impl FnMut(&str, usize, usize),
	cancelled: impl Fn() -> bool,
) -> Result<Packed> {
	let mut paths = Vec::new();
	let mut skipped = Vec::new();
	collect(folder, "", &mut paths, &mut skipped)
		.map_err(|e| Error::Io(folder.to_path_buf(), e))?;
	skipped.sort();
	if paths.is_empty() {
		return Err(Error::NoFiles);
	}
	paths.sort_by_cached_key(|p| sort_key(p));

	let total = paths.len();
	let mut entries = Vec::with_capacity(total);
	let mut offset: u64 = 0;
	let mut archive: u16 = 0;
	let mut chunk_size: u64 = 0;
	for (done, path) in paths.iter().enumerate() {
		if cancelled() {
			return Err(Error::Cancelled);
		}
		on_file(path, done, total);
		let source = folder.join(path);
		let (size, crc) = crc_of(&source).map_err(|e| Error::Io(source.clone(), e))?;
		let (ext, dir, name) = split(&path.to_lowercase());
		let (entry_archive, entry_offset) = if !options.multi_chunk {
			let at = offset;
			offset += size;
			(IN_DIR, at)
		} else if size == 0 {
			(IN_DIR, 0)
		} else {
			if chunk_size > 0 && chunk_size >= options.chunk_size {
				archive += 1;
				chunk_size = 0;
			}
			let at = chunk_size;
			chunk_size += size;
			(archive, at)
		};
		entries.push(Entry {
			source,
			ext,
			dir,
			name,
			size,
			crc,
			archive: entry_archive,
			offset: u32::try_from(entry_offset).map_err(|_| {
				Error::Io(
					output.to_path_buf(),
					io::Error::other(t!("vpk.too_big").into_owned()),
				)
			})?,
		});
	}

	let tree = build_tree(&entries);
	let dir_path = if options.multi_chunk {
		sibling(output, "dir")
	} else {
		output.to_path_buf()
	};
	let parent = dir_path.parent().unwrap_or(Path::new("."));
	std::fs::create_dir_all(parent).map_err(|e| Error::Io(parent.to_path_buf(), e))?;

	let mut packed = Packed {
		files: entries.len(),
		data_size: entries.iter().map(|e| e.size).sum(),
		skipped,
		..Packed::default()
	};
	let mut finished = Vec::new();
	if options.multi_chunk {
		let chunks = entries
			.iter()
			.filter(|e| e.archive != IN_DIR)
			.map(|e| e.archive);
		for index in 0..=chunks.max().unwrap_or(0) {
			if entries.iter().all(|e| e.archive != index) {
				continue;
			}
			let path = sibling(output, &format!("{index:03}"));
			let temp = temp_in(&path)?;
			let mut out = BufWriter::new(temp.as_file());
			for entry in entries.iter().filter(|e| e.archive == index) {
				if cancelled() {
					return Err(Error::Cancelled);
				}
				copy_file(entry, &mut out)?;
			}
			out.flush().map_err(|e| Error::Io(path.clone(), e))?;
			drop(out);
			finished.push((temp, path));
		}
	}

	let temp = temp_in(&dir_path)?;
	let io_err = |e| Error::Io(dir_path.clone(), e);
	let mut out = Md5Writer::new(BufWriter::new(temp.as_file()));
	let data_size = if options.multi_chunk {
		0
	} else {
		packed.data_size
	};
	for value in [
		SIGNATURE,
		2,
		len_u32(tree.len()),
		u32::try_from(data_size).map_err(|_| {
			Error::Io(
				dir_path.clone(),
				io::Error::other(t!("vpk.too_big").into_owned()),
			)
		})?,
		0,
		48,
		0,
	] {
		out.write_all(&value.to_le_bytes()).map_err(io_err)?;
	}
	out.write_all(&tree).map_err(io_err)?;
	if !options.multi_chunk {
		for entry in &entries {
			if cancelled() {
				return Err(Error::Cancelled);
			}
			copy_file(entry, &mut out)?;
		}
	}
	out.write_all(&Md5::digest(&tree)).map_err(io_err)?;
	out.write_all(&Md5::digest(b"")).map_err(io_err)?;
	let whole = out.digest();
	let mut inner = out.into_inner();
	inner.write_all(&whole).map_err(io_err)?;
	inner.flush().map_err(io_err)?;
	drop(inner);
	finished.push((temp, dir_path.clone()));

	for (temp, path) in finished {
		temp.persist(&path)
			.map_err(|e| Error::Io(path.clone(), e.error))?;
		packed.outputs.push(path);
	}
	packed.outputs.sort();
	Ok(packed)
}

/// `<stem>_<suffix>.vpk` next to `output`.
fn sibling(output: &Path, suffix: &str) -> PathBuf {
	let stem = output
		.file_stem()
		.map(|s| s.to_string_lossy().into_owned())
		.unwrap_or_default();
	output.with_file_name(format!("{stem}_{suffix}.vpk"))
}

fn temp_in(path: &Path) -> Result<tempfile::NamedTempFile> {
	let dir = path.parent().unwrap_or(Path::new("."));
	tempfile::Builder::new()
		.prefix(".prybar-vpk")
		.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o644))
		.tempfile_in(dir)
		.map_err(|e| Error::Io(dir.to_path_buf(), e))
}

fn len_u32(len: usize) -> u32 {
	u32::try_from(len).unwrap_or(u32::MAX)
}

/// A folder of one extension: its path and its files as entry indexes.
type Folder<'a> = (&'a str, Vec<usize>);

/// Extensions, folders, files, each in reverse order of first appearance.
fn build_tree(entries: &[Entry]) -> Vec<u8> {
	let mut exts: Vec<(&str, Vec<Folder>)> = Vec::new();
	for (index, entry) in entries.iter().enumerate() {
		let ext = match exts.iter().position(|(e, _)| *e == entry.ext) {
			Some(i) => i,
			None => {
				exts.push((&entry.ext, Vec::new()));
				exts.len() - 1
			}
		};
		let dirs = &mut exts[ext].1;
		match dirs.iter().position(|(d, _)| *d == entry.dir) {
			Some(i) => dirs[i].1.push(index),
			None => dirs.push((&entry.dir, vec![index])),
		}
	}
	let mut tree = Vec::new();
	for (ext, dirs) in exts.iter().rev() {
		push_str(&mut tree, ext);
		for (dir, files) in dirs.iter().rev() {
			push_str(&mut tree, dir);
			for &index in files.iter().rev() {
				let entry = &entries[index];
				push_str(&mut tree, &entry.name);
				tree.extend_from_slice(&entry.crc.to_le_bytes());
				tree.extend_from_slice(&0u16.to_le_bytes());
				tree.extend_from_slice(&entry.archive.to_le_bytes());
				tree.extend_from_slice(&entry.offset.to_le_bytes());
				tree.extend_from_slice(&len_u32(entry.size as usize).to_le_bytes());
				tree.extend_from_slice(&0xffffu16.to_le_bytes());
			}
			tree.push(0);
		}
		tree.push(0);
	}
	tree.push(0);
	tree
}

fn push_str(out: &mut Vec<u8>, text: &str) {
	out.extend_from_slice(text.as_bytes());
	out.push(0);
}

fn copy_file(entry: &Entry, out: &mut impl Write) -> Result<()> {
	let file =
		std::fs::File::open(&entry.source).map_err(|e| Error::Io(entry.source.clone(), e))?;
	// A file that changed since its CRC was taken would make a broken package.
	let copied = io::copy(&mut file.take(entry.size + 1), out)
		.map_err(|e| Error::Io(entry.source.clone(), e))?;
	if copied != entry.size {
		return Err(Error::Io(
			entry.source.clone(),
			io::Error::other(t!("vpk.changed").into_owned()),
		));
	}
	Ok(())
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

struct Md5Writer<W> {
	inner: W,
	hasher: Md5,
}

impl<W: Write> Md5Writer<W> {
	fn new(inner: W) -> Self {
		Self {
			inner,
			hasher: Md5::new(),
		}
	}

	fn digest(&self) -> [u8; 16] {
		self.hasher.clone().finalize().into()
	}

	fn into_inner(self) -> W {
		self.inner
	}
}

impl<W: Write> Write for Md5Writer<W> {
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

	fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
		let temp = tempfile::tempdir().unwrap();
		for (path, data) in files {
			let path = temp.path().join("src").join(path);
			std::fs::create_dir_all(path.parent().unwrap()).unwrap();
			std::fs::write(path, data).unwrap();
		}
		temp
	}

	#[test]
	fn splits_paths() {
		assert_eq!(
			split("a/b/c.dx90.vtx"),
			("vtx".into(), "a/b".into(), "c.dx90".into())
		);
		assert_eq!(split("readme"), (" ".into(), " ".into(), "readme".into()));
	}

	#[test]
	fn sorts_like_vpk() {
		let mut paths = vec![
			"sh_glide.lua",
			"shitdecals.lua",
			"x.txt",
			"x/y.txt",
			"x-y.txt",
			"X_z.txt",
		];
		paths.sort_by_cached_key(|p| sort_key(p));
		assert_eq!(
			paths,
			[
				"shitdecals.lua",
				"sh_glide.lua",
				"x/y.txt",
				"x-y.txt",
				"x.txt",
				"X_z.txt"
			]
		);
	}

	#[test]
	fn reads_back() {
		let temp = folder(&[
			("Materials/A.VMT", "vmt"),
			("readme", "text"),
			("empty.txt", ""),
			("bin/x.dll", "no"),
			(".git/config", "no"),
		]);
		let out = temp.path().join("out.vpk");
		let packed = pack(
			&temp.path().join("src"),
			&out,
			Options::default(),
			|_, _, _| {},
			|| false,
		)
		.unwrap();
		assert_eq!(packed.skipped, [".git/", "bin/x.dll"]);
		assert_eq!(packed.outputs, std::slice::from_ref(&out));
		let package = crate::package::Package::open(&out).unwrap();
		let mut names: Vec<_> = package.entries.iter().map(|e| e.path.clone()).collect();
		names.sort();
		assert_eq!(names, ["empty.txt", "materials/a.vmt", "readme"]);
	}

	#[test]
	fn splits_into_chunks() {
		let temp = folder(&[
			("a.bin", "aaaa"),
			("b.bin", "bbbb"),
			("c.bin", "cc"),
			("d.txt", ""),
		]);
		let options = Options {
			multi_chunk: true,
			chunk_size: 6,
		};
		let out = temp.path().join("pak.vpk");
		let packed = pack(
			&temp.path().join("src"),
			&out,
			options,
			|_, _, _| {},
			|| false,
		)
		.unwrap();
		let names: Vec<_> = packed
			.outputs
			.iter()
			.map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
			.collect();
		// a+b reach 8 >= 6, so c opens the second chunk.
		assert_eq!(names, ["pak_000.vpk", "pak_001.vpk", "pak_dir.vpk"]);
		assert_eq!(
			std::fs::read(temp.path().join("pak_000.vpk")).unwrap(),
			b"aaaabbbb"
		);
		assert_eq!(
			std::fs::read(temp.path().join("pak_001.vpk")).unwrap(),
			b"cc"
		);
	}

	#[test]
	fn reads_options() {
		let args = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
		assert_eq!(
			Options::from_args(true, &args(&["-c", "50"]))
				.unwrap()
				.chunk_size,
			50 << 20
		);
		assert!(Options::from_args(true, &args(&["-c"])).is_err());
		assert!(Options::from_args(false, &args(&["-P"])).is_err());
	}
}
