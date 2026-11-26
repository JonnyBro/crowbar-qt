//! Tool command lines, native or through Proton.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use rust_i18n::t;

use crate::paths::clean_path;
use crate::steam::App;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
	#[error("{}", t!("launch.missing", path = .0.display()))]
	Missing(PathBuf),
	#[error("{}", t!("launch.no_proton", name = .0))]
	NoProton(String),
	#[error("{}", t!("launch.no_prefix", name = .0))]
	NoPrefix(String),
	#[error("{}", t!("launch.no_steam"))]
	NoSteam,
	/// The game's tools folder has no such tool.
	#[error("{}", t!("launch.no_tool", name = .0, tool = .1))]
	NoTool(String, &'static str),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
	/// Output is captured.
	Console,
	/// Needs the Steam client; output is not needed.
	Gui,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
	pub program: PathBuf,
	pub args: Vec<String>,
	/// The whole environment of the process.
	pub env: BTreeMap<String, String>,
	pub working_dir: Option<PathBuf>,
}

impl Command {
	pub fn new(program: impl Into<PathBuf>, args: Vec<String>) -> Self {
		Self {
			program: program.into(),
			args,
			env: system_environment(),
			working_dir: None,
		}
	}

	/// The command as a shell would show it, for logs.
	pub fn display_string(&self) -> String {
		std::iter::once(self.program.display().to_string())
			.chain(self.args.iter().cloned())
			.map(|part| {
				if part.is_empty() || part.contains(' ') {
					format!("\"{part}\"")
				} else {
					part
				}
			})
			.collect::<Vec<_>>()
			.join(" ")
	}
}

pub fn system_environment() -> BTreeMap<String, String> {
	std::env::vars_os()
		.filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
		.collect()
}

pub fn is_windows_executable(path: &Path) -> bool {
	let mut head = [0; 2];
	std::fs::File::open(path)
		.and_then(|mut f| f.read_exact(&mut head))
		.is_ok_and(|()| &head == b"MZ")
}

/// Prefers a 64-bit build from win64, x64 or linux64.
pub fn find_tool(app: &App, name: &str) -> Option<PathBuf> {
	let tools = app.tools_path.as_ref()?;
	// 32-bit tools run out of memory on big jobs, e.g. gmad on 2 GB addons.
	["win64", "x64", "linux64", ""]
		.iter()
		.flat_map(|dir| {
			[format!("{name}.exe"), name.to_owned()]
				.into_iter()
				.map(move |file| tools.join(dir).join(file))
		})
		.find(|path| path.is_file())
}

/// Like [`find_tool`], with an error naming the game when it is missing.
pub fn require_tool(app: &App, name: &'static str) -> Result<PathBuf> {
	find_tool(app, name).ok_or_else(|| Error::NoTool(app.name.clone(), name))
}

pub fn to_windows_path(host_path: &Path) -> String {
	let absolute = std::path::absolute(host_path).unwrap_or_else(|_| host_path.to_path_buf());
	format!(
		"Z:{}",
		clean_path(&absolute).to_string_lossy().replace('/', "\\")
	)
}

/// Windows tools print paths in any case; this finds the real one.
pub fn find_path_ignoring_case(base: &Path, relative: &str) -> Option<PathBuf> {
	let mut current = base.to_path_buf();
	for part in relative.split('/').filter(|p| !p.is_empty()) {
		let exact = current.join(part);
		if exact.exists() {
			current = exact;
			continue;
		}
		let found = std::fs::read_dir(&current).ok()?.flatten().find(|entry| {
			entry
				.file_name()
				.to_string_lossy()
				.eq_ignore_ascii_case(part)
		})?;
		current = found.path();
	}
	current.exists().then(|| clean_path(&current))
}

/// Rewrites every `Z:\...` path in tool output back to a host path.
pub fn to_host_paths(text: &str) -> String {
	const STOP: &[char] = &['"', '\r', '\n', ':', '*', '?', '<', '>', '|'];
	let mut out = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(start) = rest.find(['Z', 'z']) {
		let candidate = &rest[start..];
		let bytes = candidate.as_bytes();
		if bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
			out.push_str(&rest[..start]);
			let path = &candidate[2..];
			let len = path.find(STOP).unwrap_or(path.len());
			out.push_str(&path[..len].replace('\\', "/"));
			rest = &path[len..];
		} else {
			out.push_str(&rest[..=start]);
			rest = &rest[start + 1..];
		}
	}
	out.push_str(rest);
	out
}

/// Drops terminal colour and cursor codes that some tools (steamcmd) print.
pub fn strip_terminal_codes(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut chars = text.chars().peekable();
	while let Some(c) = chars.next() {
		if c != '\x1b' {
			out.push(c);
			continue;
		}
		match chars.peek() {
			Some('[') => {
				chars.next();
				while chars
					.next_if(|c| c.is_ascii_digit() || *c == ';' || *c == '?')
					.is_some()
				{}
				chars.next_if(char::is_ascii_alphabetic);
			}
			Some('(' | ')') => {
				chars.next();
				chars.next_if(char::is_ascii_alphanumeric);
			}
			Some('=' | '>') => {
				chars.next();
			}
			_ => out.push(c),
		}
	}
	out
}

pub fn is_proton_noise(line: &str) -> bool {
	["ProtonFixes[", "ntsync:", "fsync:", "esync:"]
		.iter()
		.any(|prefix| line.starts_with(prefix))
}

fn is_env_name(name: &str) -> bool {
	let mut chars = name.chars();
	chars
		.next()
		.is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
		&& chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `NAME=value` sets, `-NAME` removes, `#` comments; returns the bad lines.
pub fn apply_environment_lines(env: &mut BTreeMap<String, String>, lines: &str) -> Vec<String> {
	let mut invalid = Vec::new();
	for raw in lines.lines() {
		let line = raw.trim();
		if line.is_empty() || line.starts_with('#') {
			continue;
		}
		if let Some(name) = line.strip_prefix('-')
			&& is_env_name(name)
		{
			env.remove(name);
			continue;
		}
		if let Some((name, value)) = line.split_once('=')
			&& is_env_name(name)
		{
			env.insert(name.to_owned(), value.to_owned());
			continue;
		}
		invalid.push(raw.to_owned());
	}
	invalid
}

/// `user_env` goes first so it can't break the Proton variables.
pub fn build(
	app: &App,
	executable: &Path,
	args: Vec<String>,
	mode: Mode,
	user_env: &str,
) -> Result<Command> {
	if !executable.exists() {
		return Err(Error::Missing(executable.to_path_buf()));
	}
	let mut command = Command::new(executable, args);
	apply_environment_lines(&mut command.env, user_env);
	if !is_windows_executable(executable) {
		return Ok(command);
	}

	let proton = app
		.proton_path
		.as_ref()
		.ok_or_else(|| Error::NoProton(app.name.clone()))?;
	let prefix = app
		.compat_data_path
		.as_ref()
		.ok_or_else(|| Error::NoPrefix(app.name.clone()))?;

	// "run" keeps the Steam API but drops stdout; "runinprefix" skips the shim.
	let verb = match mode {
		Mode::Console => "runinprefix",
		Mode::Gui => "run",
	};
	command.program = proton.join("proton");
	command.args = [verb.to_owned(), executable.display().to_string()]
		.into_iter()
		.chain(command.args)
		.collect();

	let app_id = app.id.to_string();
	let env = &mut command.env;
	env.insert(
		"STEAM_COMPAT_DATA_PATH".into(),
		prefix.display().to_string(),
	);
	env.insert(
		"STEAM_COMPAT_CLIENT_INSTALL_PATH".into(),
		app.steam_root
			.as_ref()
			.map(|root| root.display().to_string())
			.unwrap_or_default(),
	);
	env.insert("SteamAppId".into(), app_id.clone());
	env.insert("SteamGameId".into(), app_id);
	// Under Wine's Wayland driver these tools' menus close at once.
	env.remove("PROTON_ENABLE_WAYLAND");
	Ok(command)
}

/// A path argument for a tool: Z:\ form for Windows tools.
pub fn tool_path(windows: bool, host_path: &Path) -> String {
	if windows {
		to_windows_path(host_path)
	} else {
		host_path.display().to_string()
	}
}

pub fn find_executable(name: &str) -> Option<PathBuf> {
	std::env::var_os("PATH").and_then(|path| {
		std::env::split_paths(&path)
			.map(|dir| dir.join(name))
			.find(|candidate| candidate.is_file())
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn write(path: &Path, content: &[u8]) {
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		std::fs::write(path, content).unwrap();
	}

	#[test]
	fn windows_paths() {
		assert_eq!(
			to_windows_path(Path::new("/mnt/hdd/Steam Library/game")),
			"Z:\\mnt\\hdd\\Steam Library\\game"
		);
		assert_eq!(to_windows_path(Path::new("/a/b/../c/")), "Z:\\a\\c");
	}

	#[test]
	fn host_paths() {
		assert_eq!(
			to_host_paths("writing Z:\\mnt\\game\\models/a/tri.mdl:"),
			"writing /mnt/game/models/a/tri.mdl:"
		);
		assert_eq!(
			to_host_paths("gamedir: \"z:\\mnt\\my game\\\""),
			"gamedir: \"/mnt/my game/\""
		);
		assert_eq!(
			to_host_paths("Z:\\a\\b.smd (3) : bad\\token"),
			"/a/b.smd (3) : bad\\token"
		);
		assert_eq!(to_host_paths("no paths here"), "no paths here");
		assert_eq!(to_host_paths("Zebra: z:"), "Zebra: z:");
		assert_eq!(to_host_paths("Тест Z:\\д\\е"), "Тест /д/е");
	}

	#[test]
	fn terminal_codes() {
		assert_eq!(
			strip_terminal_codes("\x1b[0mSuccess. Downloaded\x1b[0m"),
			"Success. Downloaded"
		);
		assert_eq!(strip_terminal_codes("\x1b[1;31mred\x1b[K text"), "red text");
		assert_eq!(strip_terminal_codes("plain [0m stays"), "plain [0m stays");
		assert_eq!(strip_terminal_codes("\x1b(Bx\x1b=y"), "xy");
	}

	#[test]
	fn proton_noise() {
		assert!(is_proton_noise(
			"ProtonFixes[1] WARN: Skipping fix execution."
		));
		assert!(is_proton_noise("ntsync: up and running."));
		assert!(!is_proton_noise("Building binary model files..."));
	}

	#[test]
	fn environment_lines() {
		let mut env = BTreeMap::from([
			("MANGOHUD".to_owned(), "1".to_owned()),
			("KEEP".to_owned(), "yes".to_owned()),
		]);
		let invalid = apply_environment_lines(
			&mut env,
			"# comment\n\n-MANGOHUD\nDXVK_HUD=fps\n SPACED = x\nEMPTY=\n1BAD=x\n-\nnoequals",
		);
		assert!(!env.contains_key("MANGOHUD"));
		assert_eq!(env["DXVK_HUD"], "fps");
		assert_eq!(env["KEEP"], "yes");
		assert_eq!(env["EMPTY"], "");
		assert_eq!(invalid, [" SPACED = x", "1BAD=x", "-", "noequals"]);
	}

	#[test]
	fn build_commands() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path();
		let exe = dir.join("game/bin/studiomdl.exe");
		write(&exe, b"MZ\x90");
		write(&dir.join("native/tool"), b"\x7fELF");

		let mut app = App {
			id: 4000,
			name: "Test".into(),
			steam_root: Some(dir.join("steam")),
			tools_path: Some(dir.join("game/bin")),
			..App::default()
		};
		assert_eq!(
			build(&app, &exe, vec![], Mode::Console, ""),
			Err(Error::NoProton("Test".into()))
		);

		app.proton_path = Some(dir.join("proton"));
		assert_eq!(
			build(&app, &exe, vec![], Mode::Console, ""),
			Err(Error::NoPrefix("Test".into()))
		);
		app.compat_data_path = Some(dir.join("compatdata/4000"));

		let command = build(
			&app,
			&exe,
			vec!["-x".into()],
			Mode::Console,
			"MANGOHUD=0\nSteamAppId=1\nPROTON_ENABLE_WAYLAND=1",
		)
		.unwrap();
		assert_eq!(command.program, dir.join("proton/proton"));
		assert_eq!(
			command.args,
			[
				"runinprefix".to_owned(),
				exe.display().to_string(),
				"-x".into()
			]
		);
		assert_eq!(
			command.env["STEAM_COMPAT_DATA_PATH"],
			dir.join("compatdata/4000").display().to_string()
		);
		assert_eq!(command.env["MANGOHUD"], "0");
		// Proton's own variables are not something a setting may break.
		assert_eq!(command.env["SteamAppId"], "4000");
		assert!(!command.env.contains_key("PROTON_ENABLE_WAYLAND"));

		let gui = build(&app, &exe, vec![], Mode::Gui, "").unwrap();
		assert_eq!(gui.args[0], "run");

		let native = build(
			&app,
			&dir.join("native/tool"),
			vec!["-a".into()],
			Mode::Console,
			"",
		)
		.unwrap();
		assert_eq!(native.program, dir.join("native/tool"));
		assert_eq!(native.args, ["-a"]);

		assert!(matches!(
			build(&app, &dir.join("missing"), vec![], Mode::Console, ""),
			Err(Error::Missing(_))
		));
	}

	#[test]
	fn prefers_64_bit_tools() {
		let temp = tempfile::tempdir().unwrap();
		let bin = temp.path().join("bin");
		write(&bin.join("gmad.exe"), b"MZ");
		write(&bin.join("vpk.exe"), b"MZ");
		write(&bin.join("win64/gmad.exe"), b"MZ");
		// A folder, not a tool.
		std::fs::create_dir_all(bin.join("win64/vpk.exe")).unwrap();

		let mut app = App {
			tools_path: Some(bin.clone()),
			..App::default()
		};
		assert_eq!(find_tool(&app, "gmad"), Some(bin.join("win64/gmad.exe")));
		assert_eq!(find_tool(&app, "vpk"), Some(bin.join("vpk.exe")));
		assert_eq!(find_tool(&app, "hlmv"), None);

		app.tools_path = Some(bin.join("win64"));
		assert_eq!(find_tool(&app, "gmad"), Some(bin.join("win64/gmad.exe")));
	}

	#[test]
	fn path_ignoring_case() {
		let temp = tempfile::tempdir().unwrap();
		write(&temp.path().join("My Addon/Models/A.qc"), b"");
		assert_eq!(
			find_path_ignoring_case(temp.path(), "my addon/models/a.qc"),
			Some(temp.path().join("My Addon/Models/A.qc"))
		);
		assert_eq!(find_path_ignoring_case(temp.path(), "my addon/b.qc"), None);
	}

	#[test]
	fn display_string_quotes() {
		let command = Command::new("/bin/tool", vec!["a b".into(), String::new(), "c".into()]);
		assert_eq!(command.display_string(), "/bin/tool \"a b\" \"\" c");
	}
}
