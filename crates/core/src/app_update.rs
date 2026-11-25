//! New releases on GitHub, and replacing the running binary with the one from
//! a release archive.

use std::io::Read;
use std::path::{Path, PathBuf};

use rust_i18n::t;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::workshop::Client;

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/JonnyBro/prybar/releases/latest";
const ARCHIVE_SUFFIX: &str = "-linux-x86_64.tar.gz";
const SUMS: &str = "SHA256SUMS";
const BINARY: &str = "prybar";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
	/// Without the leading `v`, like `2.1.0`.
	pub version: String,
	/// The release page.
	pub url: String,
	archive: Option<Asset>,
	sums_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Asset {
	name: String,
	browser_download_url: String,
}

#[derive(Deserialize)]
struct LatestRelease {
	tag_name: String,
	html_url: String,
	#[serde(default)]
	assets: Vec<Asset>,
}

impl Release {
	/// The release has an archive this build can update itself from.
	pub fn installable(&self) -> bool {
		self.archive.is_some() && self.sums_url.is_some()
	}
}

/// The latest release if it's newer than this build; `None` if it isn't or the
/// repository has no releases yet.
pub async fn check(client: &Client) -> Result<Option<Release>, String> {
	let Some(bytes) = client.fetch_optional(LATEST_RELEASE_API).await? else {
		return Ok(None);
	};
	let latest: LatestRelease = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
	Ok(release_if_newer(latest, crate::VERSION))
}

fn release_if_newer(latest: LatestRelease, current: &str) -> Option<Release> {
	if !is_newer(&latest.tag_name, current) {
		return None;
	}
	let sums_url = latest
		.assets
		.iter()
		.find(|a| a.name == SUMS)
		.map(|a| a.browser_download_url.clone());
	let archive = latest
		.assets
		.into_iter()
		.find(|a| a.name.ends_with(ARCHIVE_SUFFIX));
	Some(Release {
		version: latest.tag_name.trim_start_matches('v').to_owned(),
		url: latest.html_url,
		archive,
		sums_url,
	})
}

/// `v2.1.0`, `2.1` or `2.1.0-beta` as numbers; anything after `-` is ignored.
fn parse_version(text: &str) -> Option<[u64; 3]> {
	let core = text
		.trim()
		.trim_start_matches('v')
		.split(['-', '+'])
		.next()?;
	let mut parts = [0u64; 3];
	for (slot, part) in parts.iter_mut().zip(core.split('.')) {
		*slot = part.parse().ok()?;
	}
	Some(parts)
}

fn is_newer(tag: &str, current: &str) -> bool {
	match (parse_version(tag), parse_version(current)) {
		(Some(tag), Some(current)) => tag > current,
		_ => false,
	}
}

/// The binary that runs now. Once it has been replaced, the kernel reports the
/// old file as "… (deleted)", but the path is still the right one.
pub fn current_exe() -> std::io::Result<PathBuf> {
	let exe = std::env::current_exe()?;
	Ok(
		match exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
			Some(path) => PathBuf::from(path),
			None => exe,
		},
	)
}

/// Whether this binary can replace itself: it lives in a folder the user can
/// write to, not in a system folder a package manager owns.
pub fn can_replace_self() -> bool {
	current_exe()
		.ok()
		.and_then(|exe| exe.parent().map(Path::to_path_buf))
		.is_some_and(|dir| tempfile::tempfile_in(dir).is_ok())
}

/// Downloads the release archive, checks it against `SHA256SUMS` and puts its
/// binary in place of the running one. The new version runs from the next start.
pub async fn install(
	client: &Client,
	release: &Release,
	work_dir: &Path,
	on_progress: impl FnMut(u64, Option<u64>),
	cancel: impl Future<Output = ()>,
) -> Result<PathBuf, String> {
	let (Some(archive), Some(sums_url)) = (&release.archive, &release.sums_url) else {
		return Err(t!("update.no_archive").into_owned());
	};
	let exe = current_exe().map_err(|e| e.to_string())?;
	let dir = exe
		.parent()
		.ok_or_else(|| t!("update.no_archive").into_owned())?;

	let sums = client
		.fetch_optional(sums_url)
		.await?
		.ok_or_else(|| t!("update.no_sum", name = archive.name).into_owned())?;
	let expected = expected_sum(&String::from_utf8_lossy(&sums), &archive.name)
		.ok_or_else(|| t!("update.no_sum", name = archive.name).into_owned())?;

	let archive_file = work_dir.join(&archive.name);
	client
		.download(
			&archive.browser_download_url,
			&archive_file,
			on_progress,
			cancel,
		)
		.await
		.map_err(|e| e.to_string())?;

	let dir = dir.to_path_buf();
	let target = exe.clone();
	tokio::task::spawn_blocking(move || {
		let actual = sha256_file(&archive_file).map_err(|e| e.to_string())?;
		if actual != expected {
			return Err(t!("update.bad_sum").into_owned());
		}
		replace_binary(&archive_file, &dir, &target)
	})
	.await
	.map_err(|e| e.to_string())??;
	Ok(exe)
}

fn expected_sum(sums: &str, name: &str) -> Option<String> {
	sums.lines().find_map(|line| {
		let (sum, file) = line.split_once(char::is_whitespace)?;
		// sha256sum marks binary mode with '*' before the name.
		let file = file.trim_start().trim_start_matches('*');
		(file == name).then(|| sum.to_ascii_lowercase())
	})
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
	let mut file = std::fs::File::open(path)?;
	let mut hasher = Sha256::new();
	let mut buf = vec![0; 1 << 16];
	loop {
		let n = file.read(&mut buf)?;
		if n == 0 {
			break;
		}
		hasher.update(&buf[..n]);
	}
	Ok(hasher
		.finalize()
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect())
}

/// Only the binary is taken from the archive; nothing else in it is written out.
fn replace_binary(archive: &Path, dir: &Path, target: &Path) -> Result<(), String> {
	use std::os::unix::fs::PermissionsExt;

	let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
	let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
	for entry in tar.entries().map_err(|e| e.to_string())? {
		let mut entry = entry.map_err(|e| e.to_string())?;
		let path = entry.path().map_err(|e| e.to_string())?.into_owned();
		// The archive holds one folder; the binary is right inside it.
		let is_binary = entry.header().entry_type().is_file()
			&& path.components().count() == 2
			&& path.file_name().is_some_and(|n| n == BINARY);
		if !is_binary {
			continue;
		}
		let mut new = tempfile::Builder::new()
			.prefix(".prybar-update")
			.tempfile_in(dir)
			.map_err(|e| e.to_string())?;
		std::io::copy(&mut entry, &mut new).map_err(|e| e.to_string())?;
		new.as_file()
			.set_permissions(std::fs::Permissions::from_mode(0o755))
			.map_err(|e| e.to_string())?;
		// A rename leaves the running process on the old file, which stays
		// readable until it exits.
		new.persist(target).map_err(|e| e.to_string())?;
		return Ok(());
	}
	Err(t!("update.no_binary").into_owned())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn compares_versions() {
		assert!(is_newer("v2.1.0", "2.0.0"));
		assert!(is_newer("v2.0.1", "2.0.0"));
		assert!(is_newer("3", "2.9.9"));
		assert!(!is_newer("v2.0.0", "2.0.0"));
		assert!(!is_newer("v2.0.0", "2.0.0-dev"));
		assert!(!is_newer("v1.9.0", "2.0.0"));
		assert!(!is_newer("nightly", "2.0.0"));
		assert_eq!(parse_version("v2.1"), Some([2, 1, 0]));
	}

	#[test]
	fn finds_assets() {
		let latest: LatestRelease = serde_json::from_str(
			r#"{"tag_name":"v9.0.0","html_url":"https://x/r","assets":[
				{"name":"SHA256SUMS","browser_download_url":"https://x/s"},
				{"name":"prybar-v9.0.0-linux-x86_64.tar.gz","browser_download_url":"https://x/a"}
			]}"#,
		)
		.unwrap();
		let release = release_if_newer(latest, "2.0.0").unwrap();
		assert_eq!(release.version, "9.0.0");
		assert!(release.installable());
	}

	#[test]
	fn reads_sums() {
		let sums = "abc  prybar-v2.tar.gz\nDEF *other\n";
		assert_eq!(
			expected_sum(sums, "prybar-v2.tar.gz").as_deref(),
			Some("abc")
		);
		assert_eq!(expected_sum(sums, "other").as_deref(), Some("def"));
		assert_eq!(expected_sum(sums, "missing"), None);
	}

	#[test]
	fn replaces_from_archive() {
		let temp = tempfile::tempdir().unwrap();
		let archive = temp.path().join("a.tar.gz");
		{
			let gz = flate2::write::GzEncoder::new(
				std::fs::File::create(&archive).unwrap(),
				flate2::Compression::fast(),
			);
			let mut tar = tar::Builder::new(gz);
			for (path, data) in [("p/README.md", &b"readme"[..]), ("p/prybar", b"new")] {
				let mut header = tar::Header::new_gnu();
				header.set_size(data.len() as u64);
				header.set_mode(0o644);
				header.set_cksum();
				tar.append_data(&mut header, path, data).unwrap();
			}
			tar.into_inner().unwrap().finish().unwrap();
		}
		let target = temp.path().join("prybar");
		std::fs::write(&target, "old").unwrap();
		replace_binary(&archive, temp.path(), &target).unwrap();
		assert_eq!(std::fs::read(&target).unwrap(), b"new");
		assert_eq!(sha256_file(&target).unwrap().len(), 64);
	}
}
