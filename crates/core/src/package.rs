//! Reading VPK (v1/v2) and GMA packages, including the LZMA-compressed GMA
//! files (`*.bin`) that old workshop items are stored as.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rust_i18n::t;
use tempfile::NamedTempFile;

use crate::paths::clean_path;

const VPK_SIGNATURE: u32 = 0x55aa_1234;
const VPK_ENTRY_TERMINATOR: u16 = 0xffff;
const VPK_DIR_ARCHIVE_INDEX: u16 = 0x7fff;
const CHUNK: usize = 1 << 20;

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("{}", t!("pkg.io", path = .0.display(), error = .1))]
	Io(PathBuf, #[source] std::io::Error),
	#[error("{}", t!("pkg.not_vpk_dir"))]
	NotVpkDir,
	#[error("{}", t!("pkg.vpk_version", version = .0))]
	VpkVersion(u32),
	#[error("{}", t!("pkg.vpk_truncated"))]
	VpkTruncated,
	#[error("{}", t!("pkg.vpk_corrupt"))]
	VpkCorrupt,
	#[error("{}", t!("pkg.not_gma"))]
	NotGma,
	#[error("{}", t!("pkg.gma_corrupt"))]
	GmaCorrupt,
	#[error("{}", t!("pkg.lzma_invalid", name = .0))]
	LzmaInvalid(String),
	#[error("{}", t!("pkg.lzma_no_gma", name = .0))]
	LzmaNoGma(String),
	#[error("{}", t!("pkg.unknown", name = .0))]
	Unknown(String),
	#[error("{}", t!("pkg.too_large", path = .0))]
	TooLarge(String),
	#[error("{}", t!("pkg.cannot_read", path = .0, archive = .1))]
	CannotRead(String, String),
	#[error("{}", t!("pkg.truncated", path = .0))]
	Truncated(String),
	#[error("{}", t!("pkg.unsafe_path", path = .0))]
	UnsafePath(String),
	#[error("{}", t!("pkg.create_folder", path = .0))]
	CreateFolder(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
	/// Forward slashes, as stored in the package.
	pub path: String,
	pub size: u64,
	pub crc: u32,
	/// Where the data lives: `preload` first, then `length` bytes at `offset` of `archive`.
	pub archive: PathBuf,
	pub offset: u64,
	pub length: u64,
	pub preload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Package {
	pub file: PathBuf,
	/// "VPK v2", "GMA v3 (LZMA)" and so on.
	pub format: String,
	/// The addon title of a GMA.
	pub title: Option<String>,
	pub entries: Vec<Entry>,
	/// Decompressed copy of an LZMA-compressed GMA; entries point into it.
	_decompressed: Option<Arc<NamedTempFile>>,
}

/// What an [`Package::extract_many`] run did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtractSummary {
	pub extracted: usize,
	pub failed: usize,
	pub cancelled: bool,
}

impl Package {
	/// For a numbered VPK part such as pak01_003.vpk, the matching pak01_dir.vpk
	/// is opened instead.
	pub fn open(file: &Path) -> Result<Self> {
		let mut head = Vec::with_capacity(13);
		File::open(file)
			.and_then(|f| f.take(13).read_to_end(&mut head))
			.map_err(|e| Error::Io(file.to_path_buf(), e))?;

		if head.starts_with(b"GMAD") {
			return read_gma(file);
		}
		if head.len() >= 4 && u32::from_le_bytes([head[0], head[1], head[2], head[3]]) == VPK_SIGNATURE
		{
			return read_vpk(file);
		}
		if looks_like_lzma(&head) {
			let name = display_name(file);
			let decompressed = decompress_lzma(file)?;
			let Ok(mut package) = read_gma(decompressed.path()) else {
				return Err(Error::LzmaNoGma(name));
			};
			package.file = file.to_path_buf();
			package.format.push_str(" (LZMA)");
			package._decompressed = Some(Arc::new(decompressed));
			return Ok(package);
		}
		if let Some(dir_file) = numbered_part_dir_file(file)
			&& dir_file.is_file()
		{
			return read_vpk(&dir_file);
		}
		Err(Error::Unknown(display_name(file)))
	}

	/// The entry's data in memory, for previews; fails for entries over `max_size` bytes.
	pub fn read(&self, entry: &Entry, max_size: u64) -> Result<Vec<u8>> {
		if entry.size > max_size {
			return Err(Error::TooLarge(entry.path.clone()));
		}
		let mut data = entry.preload.clone();
		if entry.length > 0 {
			let mut body = open_at(entry, &entry.path)?.take(entry.length);
			body.read_to_end(&mut data)
				.map_err(|_| Error::Truncated(entry.path.clone()))?;
			if data.len() as u64 != entry.preload.len() as u64 + entry.length {
				return Err(Error::Truncated(entry.path.clone()));
			}
		}
		Ok(data)
	}

	/// Writes the entry under `output_dir`; paths that would land outside it are refused.
	pub fn extract(&self, entry: &Entry, output_dir: &Path) -> Result<PathBuf> {
		let relative = safe_relative_path(&entry.path).ok_or_else(|| Error::UnsafePath(entry.path.clone()))?;
		let shown = relative.display().to_string();
		let target = output_dir.join(&relative);
		if let Some(parent) = target.parent() {
			std::fs::create_dir_all(parent).map_err(|_| Error::CreateFolder(shown.clone()))?;
		}
		let mut out = File::create(&target).map_err(|e| Error::Io(relative.clone(), e))?;
		let result = (|| -> Result<()> {
			out.write_all(&entry.preload)
				.map_err(|_| Error::Truncated(shown.clone()))?;
			if entry.length > 0 {
				let mut body = open_at(entry, &shown)?.take(entry.length);
				let copied = std::io::copy(&mut body, &mut out)
					.map_err(|_| Error::Truncated(shown.clone()))?;
				if copied != entry.length {
					return Err(Error::Truncated(shown.clone()));
				}
			}
			Ok(())
		})();
		if result.is_err() {
			drop(out);
			let _ = std::fs::remove_file(&target);
		}
		result.map(|()| target)
	}

	/// Extracts the entries at `indexes`. `on_progress(done, total)` runs every
	/// 64 entries and at the end, and can stop the run; `on_error` gets each
	/// entry that failed.
	pub fn extract_many(
		&self,
		indexes: &[usize],
		output_dir: &Path,
		mut on_progress: impl FnMut(usize, usize) -> ControlFlow<()>,
		mut on_error: impl FnMut(Error),
	) -> ExtractSummary {
		let mut summary = ExtractSummary::default();
		let total = indexes.len();
		for (i, &index) in indexes.iter().enumerate() {
			let Some(entry) = self.entries.get(index) else {
				continue;
			};
			match self.extract(entry, output_dir) {
				Ok(_) => summary.extracted += 1,
				Err(e) => {
					summary.failed += 1;
					on_error(e);
				}
			}
			if (i % 64 == 0 || i + 1 == total) && on_progress(i + 1, total).is_break() {
				summary.cancelled = i + 1 < total;
				break;
			}
		}
		summary
	}
}

/// Legacy LZMA-compressed workshop files (`*_legacy.bin`).
pub fn is_lzma_file(file: &Path) -> bool {
	let mut head = Vec::with_capacity(13);
	File::open(file)
		.and_then(|f| f.take(13).read_to_end(&mut head))
		.is_ok_and(|_| looks_like_lzma(&head))
}

pub fn decompress_lzma_file(file: &Path, output: &Path) -> Result<()> {
	let decompressed = decompress_lzma(file)?;
	let _ = std::fs::remove_file(output);
	std::fs::copy(decompressed.path(), output).map_err(|e| Error::Io(output.to_path_buf(), e))?;
	Ok(())
}

/// Legacy ".lzma" container: 0x5d properties byte, then a little-endian dictionary size.
fn looks_like_lzma(head: &[u8]) -> bool {
	head.len() >= 13 && head[0] == 0x5d && head[4] <= 0x10
}

fn decompress_lzma(file: &Path) -> Result<NamedTempFile> {
	let input = File::open(file).map_err(|e| Error::Io(file.to_path_buf(), e))?;
	let mut output = NamedTempFile::new().map_err(|e| Error::Io(std::env::temp_dir(), e))?;
	let mut writer = std::io::BufWriter::new(output.as_file_mut());
	lzma_rs::lzma_decompress(&mut BufReader::new(input), &mut writer)
		.map_err(|_| Error::LzmaInvalid(display_name(file)))?;
	writer
		.flush()
		.map_err(|e| Error::Io(std::env::temp_dir(), e))?;
	drop(writer);
	Ok(output)
}

/// `pak01_003.vpk` -> `pak01_dir.vpk`.
fn numbered_part_dir_file(file: &Path) -> Option<PathBuf> {
	let name = file.file_name()?.to_str()?;
	let lower = name.to_ascii_lowercase();
	let stem = lower.strip_suffix(".vpk")?;
	let (base, number) = stem.rsplit_once('_')?;
	if number.len() != 3 || !number.bytes().all(|b| b.is_ascii_digit()) || base.is_empty() {
		return None;
	}
	Some(file.with_file_name(format!("{}_dir.vpk", &name[..base.len()])))
}

fn read_vpk(file: &Path) -> Result<Package> {
	let io = |e| Error::Io(file.to_path_buf(), e);
	let mut dir_file = File::open(file).map_err(io)?;
	let mut head = Vec::with_capacity(28);
	(&mut dir_file).take(28).read_to_end(&mut head).map_err(io)?;
	if head.len() < 12 || le_u32(&head[0..4]) != VPK_SIGNATURE {
		return Err(Error::NotVpkDir);
	}
	let version = le_u32(&head[4..8]);
	let tree_size = le_u32(&head[8..12]);
	if version != 1 && version != 2 {
		return Err(Error::VpkVersion(version));
	}
	let header_size: u64 = if version == 1 { 12 } else { 28 };
	dir_file.seek(SeekFrom::Start(header_size)).map_err(io)?;
	let mut tree_data = Vec::new();
	dir_file
		.take(u64::from(tree_size))
		.read_to_end(&mut tree_data)
		.map_err(io)?;
	if tree_data.len() as u64 != u64::from(tree_size) {
		return Err(Error::VpkTruncated);
	}

	let stem = file
		.file_stem()
		.map(|s| s.to_string_lossy().into_owned())
		.unwrap_or_default();
	let base = if stem.to_ascii_lowercase().ends_with("_dir") {
		&stem[..stem.len() - 4]
	} else {
		&stem[..]
	};
	let embedded_data_start = header_size + u64::from(tree_size);

	// A single space stands for "empty" in all three levels of the tree.
	let blank = |text: String| if text == " " { String::new() } else { text };

	let mut entries = Vec::new();
	let mut tree = ByteReader::new(&tree_data[..]);
	let corrupt = |_| Error::VpkCorrupt;
	loop {
		let extension = tree.cstring().map_err(corrupt)?;
		if extension.is_empty() {
			break;
		}
		let extension = blank(extension);
		loop {
			let directory = tree.cstring().map_err(corrupt)?;
			if directory.is_empty() {
				break;
			}
			let directory = blank(directory);
			loop {
				let name = tree.cstring().map_err(corrupt)?;
				if name.is_empty() {
					break;
				}
				let crc = tree.u32().map_err(corrupt)?;
				let preload_size = tree.u16().map_err(corrupt)?;
				let archive_index = tree.u16().map_err(corrupt)?;
				let offset = u64::from(tree.u32().map_err(corrupt)?);
				let length = u64::from(tree.u32().map_err(corrupt)?);
				let terminator = tree.u16().map_err(corrupt)?;
				let preload = tree.bytes(usize::from(preload_size)).map_err(corrupt)?;
				if terminator != VPK_ENTRY_TERMINATOR {
					return Err(Error::VpkCorrupt);
				}

				let mut path = directory.clone();
				if !path.is_empty() {
					path.push('/');
				}
				path.push_str(&blank(name));
				if !extension.is_empty() {
					path.push('.');
					path.push_str(&extension);
				}
				let (archive, offset) = if archive_index == VPK_DIR_ARCHIVE_INDEX {
					(file.to_path_buf(), offset + embedded_data_start)
				} else {
					(
						file.with_file_name(format!("{base}_{archive_index:03}.vpk")),
						offset,
					)
				};
				entries.push(Entry {
					path,
					size: length + u64::from(preload_size),
					crc,
					archive,
					offset,
					length,
					preload,
				});
			}
		}
	}

	Ok(Package {
		file: file.to_path_buf(),
		format: format!("VPK v{version}"),
		title: None,
		entries,
		_decompressed: None,
	})
}

fn read_gma(file: &Path) -> Result<Package> {
	let input = File::open(file).map_err(|e| Error::Io(file.to_path_buf(), e))?;
	let mut reader = ByteReader::new(BufReader::with_capacity(CHUNK, input));
	let corrupt = |_| Error::GmaCorrupt;

	if reader.bytes(4).map_err(|_| Error::NotGma)? != b"GMAD" {
		return Err(Error::NotGma);
	}
	let version = reader.u8().map_err(corrupt)?;
	reader.u64().map_err(corrupt)?; // author SteamID
	reader.u64().map_err(corrupt)?; // timestamp
	if version > 1 {
		// Required content, unused by the game.
		while !reader.cstring().map_err(corrupt)?.is_empty() {}
	}
	let title = reader.cstring().map_err(corrupt)?;
	reader.cstring().map_err(corrupt)?; // description
	reader.cstring().map_err(corrupt)?; // author
	reader.u32().map_err(corrupt)?; // addon version

	let mut entries = Vec::new();
	let mut data_offset = 0u64;
	while reader.u32().map_err(corrupt)? != 0 {
		let path = reader.cstring().map_err(corrupt)?;
		let length = u64::try_from(reader.i64().map_err(corrupt)?).map_err(|_| Error::GmaCorrupt)?;
		let crc = reader.u32().map_err(corrupt)?;
		entries.push(Entry {
			path,
			size: length,
			crc,
			archive: file.to_path_buf(),
			offset: data_offset,
			length,
			preload: Vec::new(),
		});
		data_offset = data_offset.checked_add(length).ok_or(Error::GmaCorrupt)?;
	}
	for entry in &mut entries {
		entry.offset += reader.position;
	}

	Ok(Package {
		file: file.to_path_buf(),
		format: format!("GMA v{version}"),
		title: Some(title),
		entries,
		_decompressed: None,
	})
}

fn open_at(entry: &Entry, shown: &str) -> Result<File> {
	let cannot_read = || Error::CannotRead(shown.to_owned(), display_name(&entry.archive));
	let mut file = File::open(&entry.archive).map_err(|_| cannot_read())?;
	file.seek(SeekFrom::Start(entry.offset))
		.map_err(|_| cannot_read())?;
	Ok(file)
}

/// Package paths come from untrusted files; anything that would land outside
/// the output folder is rejected.
fn safe_relative_path(path: &str) -> Option<PathBuf> {
	let unified = path.replace('\\', "/");
	if unified.contains(':') || unified.starts_with('/') {
		return None;
	}
	let cleaned = clean_path(Path::new(&unified));
	let first = cleaned.components().next()?;
	if !matches!(first, std::path::Component::Normal(_)) {
		return None;
	}
	Some(cleaned)
}

fn display_name(path: &Path) -> String {
	path.file_name()
		.map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

fn le_u32(bytes: &[u8]) -> u32 {
	u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Sequential little-endian reader that counts the bytes it consumed.
struct ByteReader<R> {
	inner: R,
	position: u64,
}

impl<R: BufRead> ByteReader<R> {
	fn new(inner: R) -> Self {
		Self { inner, position: 0 }
	}

	fn array<const N: usize>(&mut self) -> std::io::Result<[u8; N]> {
		let mut buffer = [0; N];
		self.inner.read_exact(&mut buffer)?;
		self.position += N as u64;
		Ok(buffer)
	}

	fn u8(&mut self) -> std::io::Result<u8> {
		Ok(self.array::<1>()?[0])
	}

	fn u16(&mut self) -> std::io::Result<u16> {
		self.array().map(u16::from_le_bytes)
	}

	fn u32(&mut self) -> std::io::Result<u32> {
		self.array().map(u32::from_le_bytes)
	}

	fn u64(&mut self) -> std::io::Result<u64> {
		self.array().map(u64::from_le_bytes)
	}

	fn i64(&mut self) -> std::io::Result<i64> {
		self.array().map(i64::from_le_bytes)
	}

	fn bytes(&mut self, count: usize) -> std::io::Result<Vec<u8>> {
		let mut buffer = vec![0; count];
		self.inner.read_exact(&mut buffer)?;
		self.position += count as u64;
		Ok(buffer)
	}

	fn cstring(&mut self) -> std::io::Result<String> {
		let mut buffer = Vec::new();
		self.inner.read_until(0, &mut buffer)?;
		self.position += buffer.len() as u64;
		if buffer.pop() != Some(0) {
			return Err(std::io::ErrorKind::UnexpectedEof.into());
		}
		Ok(String::from_utf8_lossy(&buffer).into_owned())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn put_str(data: &mut Vec<u8>, text: &str) {
		data.extend_from_slice(text.as_bytes());
		data.push(0);
	}

	fn put_vpk_entry(
		tree: &mut Vec<u8>,
		name: &str,
		archive: u16,
		offset: u32,
		length: u32,
		preload: &[u8],
	) {
		put_str(tree, name);
		tree.extend_from_slice(&0u32.to_le_bytes());
		tree.extend_from_slice(&(preload.len() as u16).to_le_bytes());
		tree.extend_from_slice(&archive.to_le_bytes());
		tree.extend_from_slice(&offset.to_le_bytes());
		tree.extend_from_slice(&length.to_le_bytes());
		tree.extend_from_slice(&0xffffu16.to_le_bytes());
		tree.extend_from_slice(preload);
	}

	fn vpk_tree() -> Vec<u8> {
		let mut tree = Vec::new();
		put_str(&mut tree, "mdl");
		put_str(&mut tree, "models/props");
		put_vpk_entry(&mut tree, "crate", 0x7fff, 0, 5, b"");
		put_vpk_entry(&mut tree, "barrel", 1, 3, 4, b"PRE");
		put_str(&mut tree, "");
		put_str(&mut tree, "");
		put_str(&mut tree, "txt");
		put_str(&mut tree, " ");
		put_vpk_entry(&mut tree, "readme", 0x7fff, 5, 2, b"");
		put_str(&mut tree, "");
		put_str(&mut tree, "");
		put_str(&mut tree, "");
		tree
	}

	fn write_vpk(dir: &Path, version: u32) {
		let tree = vpk_tree();
		let mut data = Vec::new();
		data.extend_from_slice(&VPK_SIGNATURE.to_le_bytes());
		data.extend_from_slice(&version.to_le_bytes());
		data.extend_from_slice(&(tree.len() as u32).to_le_bytes());
		if version == 2 {
			data.extend_from_slice(&[0; 16]);
		}
		data.extend_from_slice(&tree);
		data.extend_from_slice(b"CRATEok");
		std::fs::write(dir.join("pak_dir.vpk"), data).unwrap();
		std::fs::write(dir.join("pak_001.vpk"), b"xxxBARRzzz").unwrap();
	}

	fn read(path: &Path) -> Vec<u8> {
		std::fs::read(path).unwrap_or_else(|_| b"<missing>".to_vec())
	}

	#[test]
	fn vpk() {
		for version in [1, 2] {
			let temp = tempfile::tempdir().unwrap();
			write_vpk(temp.path(), version);

			// Opening a numbered part must redirect to the directory file.
			let package = Package::open(&temp.path().join("pak_001.vpk")).unwrap();
			assert_eq!(package.format, format!("VPK v{version}"));
			let paths: Vec<_> = package.entries.iter().map(|e| e.path.as_str()).collect();
			assert_eq!(
				paths,
				["models/props/crate.mdl", "models/props/barrel.mdl", "readme.txt"]
			);
			assert_eq!(package.entries[1].size, 7);

			let out = temp.path().join("out");
			for entry in &package.entries {
				package.extract(entry, &out).unwrap();
			}
			assert_eq!(read(&out.join("models/props/crate.mdl")), b"CRATE");
			assert_eq!(read(&out.join("models/props/barrel.mdl")), b"PREBARR");
			assert_eq!(read(&out.join("readme.txt")), b"ok");
			assert_eq!(package.read(&package.entries[1], 100).unwrap(), b"PREBARR");
			assert!(matches!(
				package.read(&package.entries[1], 3),
				Err(Error::TooLarge(_))
			));
		}
	}

	fn gma_bytes() -> Vec<u8> {
		let mut gma = b"GMAD".to_vec();
		gma.push(3);
		gma.extend_from_slice(&[0; 16]);
		put_str(&mut gma, "");
		put_str(&mut gma, "My Addon");
		put_str(&mut gma, "{}");
		put_str(&mut gma, "Author");
		gma.extend_from_slice(&1i32.to_le_bytes());
		gma.extend_from_slice(&1u32.to_le_bytes());
		put_str(&mut gma, "lua/autorun/a.lua");
		gma.extend_from_slice(&5i64.to_le_bytes());
		gma.extend_from_slice(&0u32.to_le_bytes());
		gma.extend_from_slice(&2u32.to_le_bytes());
		put_str(&mut gma, "../escape.txt");
		gma.extend_from_slice(&3i64.to_le_bytes());
		gma.extend_from_slice(&0u32.to_le_bytes());
		gma.extend_from_slice(&0u32.to_le_bytes());
		gma.extend_from_slice(b"helloBAD");
		gma
	}

	#[test]
	fn gma() {
		let temp = tempfile::tempdir().unwrap();
		std::fs::write(temp.path().join("addon.gma"), gma_bytes()).unwrap();

		let package = Package::open(&temp.path().join("addon.gma")).unwrap();
		assert_eq!(package.format, "GMA v3");
		assert_eq!(package.title.as_deref(), Some("My Addon"));
		assert_eq!(package.entries.len(), 2);

		let out = temp.path().join("nested/out");
		let mut errors = Vec::new();
		let mut progress = Vec::new();
		let summary = package.extract_many(
			&[0, 1],
			&out,
			|done, total| {
				progress.push((done, total));
				ControlFlow::Continue(())
			},
			|e| errors.push(e),
		);
		assert_eq!(
			summary,
			ExtractSummary {
				extracted: 1,
				failed: 1,
				cancelled: false
			}
		);
		assert_eq!(progress, [(1, 2), (2, 2)]);
		assert!(matches!(errors[..], [Error::UnsafePath(_)]));
		assert_eq!(read(&out.join("lua/autorun/a.lua")), b"hello");
		assert!(!temp.path().join("nested/escape.txt").exists());
	}

	#[test]
	fn extraction_can_be_cancelled() {
		let temp = tempfile::tempdir().unwrap();
		std::fs::write(temp.path().join("addon.gma"), gma_bytes()).unwrap();
		let package = Package::open(&temp.path().join("addon.gma")).unwrap();
		let summary = package.extract_many(
			&[0, 1],
			&temp.path().join("out"),
			|_, _| ControlFlow::Break(()),
			|_| {},
		);
		assert!(summary.cancelled);
		assert_eq!(summary.extracted, 1);
	}

	#[test]
	fn lzma_gma() {
		let temp = tempfile::tempdir().unwrap();
		let mut compressed = Vec::new();
		lzma_rs::lzma_compress(&mut &gma_bytes()[..], &mut compressed).unwrap();
		let file = temp.path().join("123_legacy.bin");
		std::fs::write(&file, compressed).unwrap();

		assert!(is_lzma_file(&file));
		let package = Package::open(&file).unwrap();
		assert_eq!(package.format, "GMA v3 (LZMA)");
		assert_eq!(package.file, file);
		assert_eq!(package.read(&package.entries[0], 100).unwrap(), b"hello");

		let output = temp.path().join("plain.gma");
		decompress_lzma_file(&file, &output).unwrap();
		assert_eq!(read(&output), gma_bytes());
	}

	#[test]
	fn rejects_other_files() {
		let temp = tempfile::tempdir().unwrap();
		std::fs::write(temp.path().join("a.vpk"), "not a package").unwrap();
		assert!(matches!(
			Package::open(&temp.path().join("a.vpk")),
			Err(Error::Unknown(_))
		));
		assert!(Package::open(&temp.path().join("missing.vpk")).is_err());

		let mut truncated = Vec::new();
		truncated.extend_from_slice(&VPK_SIGNATURE.to_le_bytes());
		truncated.extend_from_slice(&1u32.to_le_bytes());
		truncated.extend_from_slice(&500u32.to_le_bytes());
		std::fs::write(temp.path().join("b.vpk"), truncated).unwrap();
		assert!(matches!(
			Package::open(&temp.path().join("b.vpk")),
			Err(Error::VpkTruncated)
		));
	}

	#[test]
	fn unsafe_paths() {
		assert_eq!(
			safe_relative_path("a\\b/../c.txt"),
			Some(PathBuf::from("a/c.txt"))
		);
		for path in ["", ".", "..", "../x", "a/../../x", "/etc/passwd", "C:/x", "\\\\server\\x"] {
			assert_eq!(safe_relative_path(path), None, "{path}");
		}
	}

	#[test]
	fn numbered_parts() {
		let dir = |name: &str| numbered_part_dir_file(Path::new(name));
		assert_eq!(dir("/x/Pak01_003.VPK"), Some(PathBuf::from("/x/Pak01_dir.vpk")));
		assert_eq!(dir("/x/pak01_dir.vpk"), None);
		assert_eq!(dir("/x/pak01_03.vpk"), None);
		assert_eq!(dir("/x/_003.vpk"), None);
	}
}
