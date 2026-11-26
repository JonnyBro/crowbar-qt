//! Workshop IDs, Web API details, Steam's cache, steamcmd and direct downloads.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};

use rust_i18n::t;
use serde_json::Value;

use crate::launcher::{self, Command};
use crate::package;
use crate::paths::clean_path;

const DETAILS_URL: &str =
	"https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/";
const COLLECTION_URL: &str =
	"https://api.steampowered.com/ISteamRemoteStorage/GetCollectionDetails/v1/";

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("{}", t!("ws.no_details"))]
	NoDetails,
	#[error("{}", t!("ws.network", error = .0))]
	Network(String),
	#[error("{}", t!("ws.no_steamcmd"))]
	NoSteamCmd,
	#[error("{}", t!("ws.create_failed", path = .0.display()))]
	CreateFailed(PathBuf),
	#[error("{}", t!("ws.write_failed", path = .0.display()))]
	WriteFailed(PathBuf),
	#[error("{}", t!("ws.copy_failed", source = .0.display(), target = .1.display()))]
	CopyFailed(PathBuf, PathBuf),
	#[error(transparent)]
	Package(#[from] package::Error),
	#[error("{}", t!("ws.cancelled"))]
	Cancelled,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDetails {
	pub id: u64,
	pub app_id: u32,
	pub title: String,
	pub file_name: String,
	/// Only old items have a direct link.
	pub file_url: String,
	pub preview_url: String,
	pub file_size: u64,
	/// Seconds since the Unix epoch.
	pub updated: i64,
	pub error: Option<String>,
}

impl ItemDetails {
	pub fn is_valid(&self) -> bool {
		self.error.is_none() && self.id != 0
	}
}

/// A bare ID or any workshop/community URL with `?id=`.
pub fn parse_item_id(text: &str) -> Option<u64> {
	let trimmed = text.trim();
	if let Ok(id) = trimmed.parse() {
		return Some(id);
	}
	["?id=", "&id="].iter().find_map(|marker| {
		let start = trimmed.find(marker)? + marker.len();
		let digits: String = trimmed[start..]
			.chars()
			.take_while(char::is_ascii_digit)
			.collect();
		digits.parse().ok()
	})
}

/// Links and bare IDs, without duplicates.
pub fn parse_item_ids(text: &str) -> Vec<u64> {
	let mut ids = Vec::new();
	for part in text.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
		if let Some(id) = parse_item_id(part).filter(|id| *id != 0)
			&& !ids.contains(&id)
		{
			ids.push(id);
		}
	}
	ids
}

pub fn item_url(id: u64) -> String {
	format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}")
}

fn number(value: &Value) -> Option<u64> {
	value
		.as_u64()
		.or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

/// The collection's items, and whether the ID was a collection.
pub fn parse_collection(json: &[u8]) -> (Vec<u64>, bool) {
	let root: Value = serde_json::from_slice(json).unwrap_or_default();
	let collection = &root["response"]["collectiondetails"][0];
	let found = collection["result"].as_i64() == Some(1);
	// filetype 0 is a workshop item; nested collections (2) are not followed.
	let children = collection["children"]
		.as_array()
		.into_iter()
		.flatten()
		.filter(|child| child["filetype"].as_i64() == Some(0))
		.filter_map(|child| number(&child["publishedfileid"]))
		.collect();
	(children, found)
}

fn details_from(item: &Value) -> ItemDetails {
	let id = number(&item["publishedfileid"]).unwrap_or(0);
	if item["result"].as_i64() != Some(1) {
		return ItemDetails {
			id,
			error: Some(t!("ws.not_found", id = id).into_owned()),
			..ItemDetails::default()
		};
	}
	let text = |key: &str| item[key].as_str().unwrap_or_default().to_owned();
	ItemDetails {
		id,
		app_id: number(&item["consumer_app_id"])
			.and_then(|id| u32::try_from(id).ok())
			.unwrap_or(0),
		title: text("title"),
		file_name: text("filename"),
		file_url: text("file_url"),
		preview_url: text("preview_url"),
		file_size: number(&item["file_size"]).unwrap_or(0),
		updated: item["time_updated"].as_i64().unwrap_or(0),
		error: None,
	}
}

/// Every item in a GetPublishedFileDetails response, in order; failed ones carry an error.
pub fn parse_details_list(json: &[u8]) -> Vec<ItemDetails> {
	let root: Value = serde_json::from_slice(json).unwrap_or_default();
	root["response"]["publishedfiledetails"]
		.as_array()
		.into_iter()
		.flatten()
		.map(details_from)
		.collect()
}

pub fn parse_details(json: &[u8]) -> ItemDetails {
	parse_details_list(json)
		.into_iter()
		.next()
		.unwrap_or_else(|| ItemDetails {
			error: Some(Error::NoDetails.to_string()),
			..ItemDetails::default()
		})
}

/// Files Steam already downloaded for a subscribed item, from the first library that has them.
pub fn find_cached_files(libraries: &[PathBuf], app_id: u32, id: u64) -> Vec<PathBuf> {
	libraries
		.iter()
		.map(|library| {
			let dir = library.join(format!("steamapps/workshop/content/{app_id}/{id}"));
			let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
				.map(|read| {
					read.flatten()
						.map(|e| e.path())
						.filter(|p| p.is_file())
						.collect()
				})
				.unwrap_or_default();
			files.sort();
			files
		})
		.find(|files| !files.is_empty())
		.unwrap_or_default()
}

pub fn find_steamcmd() -> Option<PathBuf> {
	if let Some(path) = launcher::find_executable("steamcmd") {
		return Some(path);
	}
	let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
	[".steam/steamcmd/steamcmd.sh", "steamcmd/steamcmd.sh"]
		.iter()
		.map(|candidate| home.join(candidate))
		.find(|path| path.is_file())
}

/// With the real HOME steamcmd rewrites Steam's own files, so it runs in `home_dir`.
pub fn steamcmd_command(
	steamcmd: Option<&Path>,
	home_dir: &Path,
	install_dir: &Path,
	app_id: u32,
	id: u64,
) -> Result<Command> {
	steamcmd_command_in(
		launcher::system_environment(),
		steamcmd,
		home_dir,
		install_dir,
		app_id,
		id,
	)
}

fn steamcmd_command_in(
	mut env: BTreeMap<String, String>,
	steamcmd: Option<&Path>,
	home_dir: &Path,
	install_dir: &Path,
	app_id: u32,
	id: u64,
) -> Result<Command> {
	let steamcmd = steamcmd.ok_or(Error::NoSteamCmd)?;
	std::fs::create_dir_all(home_dir).map_err(|_| Error::CreateFailed(home_dir.to_path_buf()))?;
	let home = clean_path(home_dir);
	let home_text = home.display().to_string();

	// Variables from a running Steam session would point it back at the real client.
	env.retain(|name, _| !name.starts_with("STEAM_") && !name.starts_with("Steam"));
	env.insert("HOME".into(), home_text.clone());
	env.insert("XDG_DATA_HOME".into(), format!("{home_text}/.local/share"));
	env.insert("XDG_CONFIG_HOME".into(), format!("{home_text}/.config"));
	env.insert("XDG_CACHE_HOME".into(), format!("{home_text}/.cache"));
	env.insert("XDG_STATE_HOME".into(), format!("{home_text}/.local/state"));

	Ok(Command {
		program: steamcmd.to_path_buf(),
		args: vec![
			"+force_install_dir".into(),
			install_dir.display().to_string(),
			"+login".into(),
			"anonymous".into(),
			"+workshop_download_item".into(),
			app_id.to_string(),
			id.to_string(),
			"+quit".into(),
		],
		env,
		working_dir: Some(home),
	})
}

/// From `Success. Downloaded item ... to "<path>"`: a file or a folder.
pub fn parse_steamcmd_path(line: &str) -> Option<String> {
	let rest = &line[line.find("Success. Downloaded item ")? + "Success. Downloaded item ".len()..];
	let digits = rest.chars().take_while(char::is_ascii_digit).count();
	if digits == 0 {
		return None;
	}
	let quoted = rest[digits..].strip_prefix(" to \"")?;
	let end = quoted.find('"').filter(|&end| end > 0)?;
	Some(quoted[..end].to_owned())
}

/// A safe file name from an item: "<title> (<id>)", or the bare ID.
pub fn file_base_name(details: &ItemDetails, with_title: bool) -> String {
	let title = details
		.title
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ");
	if !with_title || title.is_empty() {
		return details.id.to_string();
	}
	let safe: String = title
		.chars()
		.map(|c| {
			if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
				'_'
			} else {
				c
			}
		})
		.take(120)
		.collect();
	format!("{} ({})", safe.trim(), details.id)
}

/// With `convert`, LZMA files are unpacked and addons become .gma.
pub fn save_file(
	file: &Path,
	output_dir: &Path,
	base_name: &str,
	convert: bool,
) -> Result<PathBuf> {
	std::fs::create_dir_all(output_dir)
		.map_err(|_| Error::CreateFailed(output_dir.to_path_buf()))?;

	if convert && package::is_lzma_file(file) {
		let temp = output_dir.join(format!(".{base_name}.partial"));
		package::decompress_lzma_file(file, &temp)?;
		let mut head = [0; 4];
		let gma = std::fs::File::open(&temp)
			.and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
			.is_ok_and(|()| &head == b"GMAD");
		let target = output_dir.join(format!("{base_name}.{}", if gma { "gma" } else { "bin" }));
		let _ = std::fs::remove_file(&target);
		if std::fs::rename(&temp, &target).is_err() {
			let _ = std::fs::remove_file(&temp);
			return Err(Error::WriteFailed(target));
		}
		return Ok(target);
	}

	let target = match file.extension() {
		Some(extension) => output_dir.join(format!("{base_name}.{}", extension.to_string_lossy())),
		None => output_dir.join(base_name),
	};
	if matches!((file.canonicalize(), target.canonicalize()), (Ok(a), Ok(b)) if a == b) {
		return Ok(target);
	}
	let _ = std::fs::remove_file(&target);
	std::fs::copy(file, &target)
		.map_err(|_| Error::CopyFailed(file.to_path_buf(), target.clone()))?;
	Ok(target)
}

/// The Web API and plain downloads.
#[derive(Clone)]
pub struct Client {
	http: reqwest::Client,
}

impl Client {
	pub fn new() -> Result<Self> {
		let http = reqwest::Client::builder()
			.user_agent(concat!("prybar/", env!("CARGO_PKG_VERSION")))
			.build()
			.map_err(|e| Error::Network(e.to_string()))?;
		Ok(Self { http })
	}

	async fn post_form(&self, url: &str, form: &[(String, String)]) -> Result<Vec<u8>> {
		let response = self
			.http
			.post(url)
			.form(form)
			.send()
			.await
			.and_then(reqwest::Response::error_for_status)
			.map_err(|e| Error::Network(e.to_string()))?;
		let bytes = response
			.bytes()
			.await
			.map_err(|e| Error::Network(e.to_string()))?;
		Ok(bytes.to_vec())
	}

	/// Details of every item, in the order asked; failed ones carry an error.
	pub async fn fetch_details(&self, ids: &[u64]) -> Result<Vec<ItemDetails>> {
		let mut form = vec![("itemcount".to_owned(), ids.len().to_string())];
		form.extend(
			ids.iter()
				.enumerate()
				.map(|(i, id)| (format!("publishedfileids[{i}]"), id.to_string())),
		);
		Ok(parse_details_list(
			&self.post_form(DETAILS_URL, &form).await?,
		))
	}

	/// The items of a collection, and whether `id` was one.
	pub async fn fetch_collection(&self, id: u64) -> Result<(Vec<u64>, bool)> {
		let form = [
			("collectioncount".to_owned(), "1".to_owned()),
			("publishedfileids[0]".to_owned(), id.to_string()),
		];
		Ok(parse_collection(
			&self.post_form(COLLECTION_URL, &form).await?,
		))
	}

	/// `None` for 404; for hosts other than Steam.
	pub async fn fetch_optional(&self, url: &str) -> std::result::Result<Option<Vec<u8>>, String> {
		let response = self.http.get(url).send().await.map_err(|e| e.to_string())?;
		if response.status() == reqwest::StatusCode::NOT_FOUND {
			return Ok(None);
		}
		let response = response.error_for_status().map_err(|e| e.to_string())?;
		let bytes = response.bytes().await.map_err(|e| e.to_string())?;
		Ok(Some(bytes.to_vec()))
	}

	/// Small downloads kept in memory, e.g. preview images.
	pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>> {
		let response = self
			.http
			.get(url)
			.send()
			.await
			.and_then(reqwest::Response::error_for_status)
			.map_err(|e| Error::Network(e.to_string()))?;
		Ok(response
			.bytes()
			.await
			.map_err(|e| Error::Network(e.to_string()))?
			.to_vec())
	}

	/// Through a temp file; `on_progress(received, total)` runs after each chunk.
	pub async fn download(
		&self,
		url: &str,
		file: &Path,
		mut on_progress: impl FnMut(u64, Option<u64>),
		cancel: impl Future<Output = ()>,
	) -> Result<()> {
		use std::io::Write;

		let partial = file.with_extension("partial");
		let result = async {
			let mut response = self
				.http
				.get(url)
				.send()
				.await
				.and_then(reqwest::Response::error_for_status)
				.map_err(|e| Error::Network(e.to_string()))?;
			let total = response.content_length();
			let mut out =
				std::fs::File::create(&partial).map_err(|_| Error::WriteFailed(partial.clone()))?;
			let mut received = 0;
			while let Some(chunk) = response
				.chunk()
				.await
				.map_err(|e| Error::Network(e.to_string()))?
			{
				out.write_all(&chunk)
					.map_err(|_| Error::WriteFailed(partial.clone()))?;
				received += chunk.len() as u64;
				on_progress(received, total);
			}
			out.flush()
				.map_err(|_| Error::WriteFailed(partial.clone()))?;
			Ok::<(), Error>(())
		};
		let outcome = tokio::select! {
			outcome = result => outcome,
			() = cancel => Err(Error::Cancelled),
		};
		match outcome {
			Ok(()) => {
				std::fs::rename(&partial, file).map_err(|_| Error::WriteFailed(file.to_path_buf()))
			}
			Err(e) => {
				let _ = std::fs::remove_file(&partial);
				Err(e)
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn write(path: &Path, content: &[u8]) {
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		std::fs::write(path, content).unwrap();
	}

	fn read(path: &Path) -> Vec<u8> {
		std::fs::read(path).unwrap_or_else(|_| b"<missing>".to_vec())
	}

	#[test]
	fn item_ids() {
		assert_eq!(parse_item_id(" 104603291 "), Some(104_603_291));
		assert_eq!(
			parse_item_id(
				"https://steamcommunity.com/sharedfiles/filedetails/?id=104603291&searchtext="
			),
			Some(104_603_291)
		);
		assert_eq!(
			parse_item_id("https://steamcommunity.com/workshop/filedetails/?l=en&id=42"),
			Some(42)
		);
		assert_eq!(parse_item_id("not an id"), None);
	}

	#[test]
	fn item_id_lists() {
		assert_eq!(
			parse_item_ids(
				"104603291 https://steamcommunity.com/sharedfiles/filedetails/?id=42, 7;\n104603291"
			),
			[104_603_291, 42, 7]
		);
		assert!(parse_item_ids("  nothing here ").is_empty());
	}

	#[test]
	fn collections() {
		let (children, is_collection) = parse_collection(
			br#"{"response":{"collectiondetails":[{"publishedfileid":"9","result":1,"children":[{"publishedfileid":"1","filetype":0},{"publishedfileid":"2","filetype":2},{"publishedfileid":"3","filetype":0}]}]}}"#,
		);
		assert!(is_collection);
		assert_eq!(children, [1, 3]);
		let (children, is_collection) = parse_collection(
			br#"{"response":{"collectiondetails":[{"publishedfileid":"5","result":9}]}}"#,
		);
		assert!(children.is_empty());
		assert!(!is_collection);
	}

	#[test]
	fn details() {
		let ok = parse_details(
			br#"{"response":{"result":1,"resultcount":1,"publishedfiledetails":[
            {"publishedfileid":"104603291","result":1,"consumer_app_id":4000,"title":"Extended Spawnmenu",
             "filename":"","file_url":"","file_size":"42347","time_updated":1785371010}]}}"#,
		);
		assert!(ok.is_valid());
		assert_eq!(ok.app_id, 4000);
		assert_eq!(ok.title, "Extended Spawnmenu");
		assert_eq!(ok.file_size, 42347);
		assert_eq!(ok.updated, 1_785_371_010);
		assert!(ok.file_url.is_empty());

		let list = parse_details_list(
			br#"{"response":{"publishedfiledetails":[{"publishedfileid":"1","result":1,"title":"A","preview_url":"https://x/a.jpg"},{"publishedfileid":"2","result":9}]}}"#,
		);
		assert_eq!(list.len(), 2);
		assert_eq!(list[0].preview_url, "https://x/a.jpg");
		assert!(list[0].is_valid());
		assert!(!list[1].is_valid());
		assert_eq!(list[1].id, 2);

		assert!(
			!parse_details(
				br#"{"response":{"publishedfiledetails":[{"publishedfileid":"5","result":9}]}}"#
			)
			.is_valid()
		);
		assert!(!parse_details(b"garbage").is_valid());
	}

	#[test]
	fn names() {
		let details = ItemDetails {
			id: 7,
			title: "  MW 2019: Rigs | <Compressed>\n512x512 ".into(),
			..ItemDetails::default()
		};
		assert_eq!(
			file_base_name(&details, true),
			"MW 2019_ Rigs _ _Compressed_ 512x512 (7)"
		);
		assert_eq!(file_base_name(&details, false), "7");
		assert_eq!(
			parse_steamcmd_path("Success. Downloaded item 42 to \"/x/content/4000/42\" (10 bytes)")
				.as_deref(),
			Some("/x/content/4000/42")
		);
		assert_eq!(parse_steamcmd_path("ERROR! Download item 42 failed"), None);
	}

	#[test]
	fn steamcmd_runs_in_its_own_home() {
		let temp = tempfile::tempdir().unwrap();
		let env = BTreeMap::from([
			(
				"STEAM_COMPAT_CLIENT_INSTALL_PATH".to_owned(),
				"/home/user/.local/share/Steam".to_owned(),
			),
			("SteamAppId".to_owned(), "4000".to_owned()),
			("PATH".to_owned(), "/usr/bin".to_owned()),
		]);
		let home = temp.path().join("home");
		let out = temp.path().join("out");
		let command = steamcmd_command_in(
			env,
			Some(Path::new("/usr/bin/steamcmd")),
			&home,
			&out,
			4000,
			42,
		)
		.unwrap();
		assert_eq!(command.env["HOME"], home.display().to_string());
		assert_eq!(
			command.env["XDG_DATA_HOME"],
			format!("{}/.local/share", home.display())
		);
		assert!(!command.env.contains_key("STEAM_COMPAT_CLIENT_INSTALL_PATH"));
		assert!(!command.env.contains_key("SteamAppId"));
		assert_eq!(command.env["PATH"], "/usr/bin");
		assert!(home.is_dir());
		assert_eq!(
			command.args[..2],
			["+force_install_dir".to_owned(), out.display().to_string()]
		);
		assert!(matches!(
			steamcmd_command(None, &home, temp.path(), 4000, 42),
			Err(Error::NoSteamCmd)
		));
	}

	#[test]
	fn cache_and_save() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path();
		let lib = dir.join("lib");
		let cached = lib.join("steamapps/workshop/content/4000/42/addon.gma");
		write(&cached, b"GMAD plain");
		assert_eq!(
			find_cached_files(&[dir.join("empty"), lib.clone()], 4000, 42),
			std::slice::from_ref(&cached)
		);
		assert!(find_cached_files(&[lib], 4000, 43).is_empty());

		let out = dir.join("out");
		assert_eq!(
			save_file(&cached, &out, "Name (42)", true).unwrap(),
			out.join("Name (42).gma")
		);
		assert_eq!(read(&out.join("Name (42).gma")), b"GMAD plain");

		// An LZMA-compressed GMA becomes a plain .gma; other compressed data stays .bin.
		for (name, content) in [
			("plain.gma.lzma", &b"GMAD compressed addon"[..]),
			("dupe.txt.lzma", &b"DUP3 not an addon"[..]),
		] {
			let mut compressed = Vec::new();
			lzma_rs::lzma_compress(&mut &content[..], &mut compressed).unwrap();
			write(&dir.join(name), &compressed);
		}
		assert_eq!(
			save_file(&dir.join("plain.gma.lzma"), &out, "a", true).unwrap(),
			out.join("a.gma")
		);
		assert_eq!(read(&out.join("a.gma")), b"GMAD compressed addon");
		assert_eq!(
			save_file(&dir.join("dupe.txt.lzma"), &out, "b", true).unwrap(),
			out.join("b.bin")
		);
		assert_eq!(read(&out.join("b.bin")), b"DUP3 not an addon");
		assert_eq!(
			save_file(&dir.join("dupe.txt.lzma"), &out, "c", false).unwrap(),
			out.join("c.lzma")
		);
	}
}
