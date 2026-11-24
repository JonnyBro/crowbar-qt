//! Compiling models with the game's studiomdl.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rust_i18n::t;

use crate::fsutil::move_file;
use crate::launcher::{self, Command, Error, Mode, Result};
use crate::paths::clean_path;
use crate::steam::App;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
	pub no_p4: bool,
	pub verbose: bool,
	/// Passed to studiomdl as they are.
	pub extra: Vec<String>,
}

impl Default for Options {
	fn default() -> Self {
		Self {
			no_p4: true,
			verbose: false,
			extra: Vec::new(),
		}
	}
}

pub fn compile_command(
	app: &App,
	game_dir: &Path,
	qc: &Path,
	options: &Options,
	user_env: &str,
) -> Result<Command> {
	let executable = launcher::require_tool(app, "studiomdl")?;
	if !qc.is_file() {
		return Err(Error::Missing(qc.to_path_buf()));
	}
	let qc = std::path::absolute(qc).unwrap_or_else(|_| qc.to_path_buf());

	let windows = launcher::is_windows_executable(&executable);
	let mut args = vec!["-game".to_owned(), launcher::tool_path(windows, game_dir)];
	if options.no_p4 {
		args.push("-nop4".into());
	}
	if options.verbose {
		args.push("-verbose".into());
	}
	args.extend(options.extra.iter().cloned());
	// Relative name plus working directory, as studiomdl resolves $include and SMD paths from there.
	args.push(
		qc.file_name()
			.map(|name| name.to_string_lossy().into_owned())
			.unwrap_or_default(),
	);

	let mut command = launcher::build(app, &executable, args, Mode::Console, user_env)?;
	command.working_dir = qc.parent().map(Path::to_path_buf);
	Ok(command)
}

/// The value of `$modelname`, with forward slashes and a .mdl extension; `None`
/// if the QC and the files it includes have none.
pub fn read_model_name(qc: &Path) -> Option<String> {
	let name = find_model_name(qc, &mut HashSet::new())?.replace('\\', "/");
	let name = clean_path(Path::new(&name)).to_string_lossy().into_owned();
	if name.is_empty() {
		return None;
	}
	Some(if name.to_ascii_lowercase().ends_with(".mdl") {
		name
	} else {
		format!("{name}.mdl")
	})
}

/// Follows `$include` files too, since many QCs keep `$modelname` in a shared .qci.
fn find_model_name(qc: &Path, visited: &mut HashSet<PathBuf>) -> Option<String> {
	let key = qc.canonicalize().ok()?;
	if visited.len() > 32 || !visited.insert(key) {
		return None;
	}
	let bytes = std::fs::read(qc).ok()?;
	let text = String::from_utf8_lossy(&bytes);

	if let Some(name) = text
		.lines()
		.find_map(|line| command_value(line, "$modelname"))
	{
		return Some(name.to_owned()).filter(|n| !n.is_empty());
	}
	let base = qc.parent().unwrap_or(Path::new("/"));
	text.lines()
		.filter_map(|line| command_value(line, "$include"))
		.filter_map(|path| launcher::find_path_ignoring_case(base, &path.replace('\\', "/")))
		.find_map(|included| find_model_name(&included, visited))
}

/// The argument of a QC command at the start of `line`: a quoted string or the
/// next word.
fn command_value<'a>(line: &'a str, command: &str) -> Option<&'a str> {
	let line = line.trim_start();
	let head = line.get(..command.len())?;
	if !head.eq_ignore_ascii_case(command) {
		return None;
	}
	let rest = &line[command.len()..];
	if !rest.starts_with(char::is_whitespace) {
		return None;
	}
	let rest = rest.trim_start();
	if let Some(quoted) = rest.strip_prefix('"') {
		return quoted.find('"').map(|end| &quoted[..end]);
	}
	rest.split_whitespace().next()
}

/// Where a compiled model ends up: under `output_dir` when its files were moved
/// there, otherwise in the game folder.
pub fn compiled_model(
	game_dir: &Path,
	output_dir: Option<&Path>,
	model_name: &str,
) -> Option<PathBuf> {
	let base = output_dir.unwrap_or(game_dir);
	launcher::find_path_ignoring_case(base, &format!("models/{model_name}"))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoveResult {
	/// False if nothing could be moved.
	pub ok: bool,
	pub log: Vec<String>,
}

/// studiomdl can only write into `<game_dir>/models`, so a different output
/// folder is done the way Crowbar does it: move the compiled files afterwards,
/// keeping the models/<subpath> layout.
pub fn move_compiled_model(game_dir: &Path, model_name: &str, output_dir: &Path) -> MoveResult {
	let mut result = MoveResult::default();
	let mut moved = 0;
	let model = Path::new(model_name);
	let subpath = model
		.parent()
		.map(|p| p.to_string_lossy().into_owned())
		.unwrap_or_default();
	let Some(source_dir) =
		launcher::find_path_ignoring_case(game_dir, &format!("models/{subpath}"))
	else {
		result.log.push(
			t!(
				"studio.folder_not_found",
				game = game_dir.display(),
				subpath = subpath
			)
			.into_owned(),
		);
		return result;
	};

	let target_dir = clean_path(&output_dir.join("models").join(&subpath));
	if same_file(&target_dir, &source_dir) {
		result.ok = true;
		return result;
	}
	if std::fs::create_dir_all(&target_dir).is_err() {
		result
			.log
			.push(t!("studio.create_failed", path = target_dir.display()).into_owned());
		return result;
	}

	const EXTENSIONS: [&str; 5] = ["ani", "mdl", "phy", "vtx", "vvd"];
	let prefix = format!(
		"{}.",
		model
			.file_stem()
			.map(|s| s.to_string_lossy().to_lowercase())
			.unwrap_or_default()
	);
	let mut files: Vec<PathBuf> = std::fs::read_dir(&source_dir)
		.map(|read| {
			read.flatten()
				.map(|e| e.path())
				.filter(|p| p.is_file())
				.collect()
		})
		.unwrap_or_default();
	files.sort();
	for file in files {
		let name = file
			.file_name()
			.map(|n| n.to_string_lossy().into_owned())
			.unwrap_or_default();
		let lower = name.to_lowercase();
		let extension = lower.rsplit('.').next().unwrap_or_default();
		if !lower.starts_with(&prefix) || !EXTENSIONS.contains(&extension) {
			continue;
		}
		match move_file(&file, &target_dir.join(&name)) {
			Ok(()) => {
				moved += 1;
				result
					.log
					.push(t!("fs.moved", name = name, folder = target_dir.display()).into_owned());
			}
			Err(_) => result.log.push(
				t!(
					"fs.move_failed",
					source = file.display(),
					target = target_dir.display()
				)
				.into_owned(),
			),
		}
	}

	if moved == 0 {
		result.log.push(
			t!(
				"studio.no_compiled",
				model = model_name,
				folder = source_dir.display()
			)
			.into_owned(),
		);
	}
	// Drops the subfolders the move emptied, but never "models" itself.
	let mut emptied = source_dir;
	for _ in Path::new(&subpath).components() {
		if std::fs::remove_dir(&emptied).is_err() {
			break;
		}
		if !emptied.pop() {
			break;
		}
	}
	result.ok = moved > 0;
	result
}

fn same_file(a: &Path, b: &Path) -> bool {
	match (a.canonicalize(), b.canonicalize()) {
		(Ok(a), Ok(b)) => a == b,
		_ => a == b,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use launcher::to_windows_path;

	fn write(path: &Path, content: &[u8]) {
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		std::fs::write(path, content).unwrap();
	}

	fn names(dir: &Path) -> Vec<String> {
		let mut names: Vec<_> = std::fs::read_dir(dir)
			.unwrap()
			.flatten()
			.filter(|e| e.path().is_file())
			.map(|e| e.file_name().to_string_lossy().into_owned())
			.collect();
		names.sort();
		names
	}

	#[test]
	fn compile_commands() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path();
		let exe = dir.join("game/bin/studiomdl.exe");
		write(&exe, b"MZ\x90");
		write(&dir.join("src/model.qc"), b"");

		let mut app = App {
			id: 4000,
			name: "Test".into(),
			tools_path: Some(dir.join("game/bin")),
			..App::default()
		};
		let qc = dir.join("src/model.qc");
		assert!(matches!(
			compile_command(&app, &dir.join("game/hl2"), &qc, &Options::default(), ""),
			Err(Error::NoProton(_))
		));

		app.proton_path = Some(dir.join("proton"));
		app.compat_data_path = Some(dir.join("compatdata/4000"));
		let options = Options {
			verbose: true,
			extra: vec!["-nox360".into()],
			..Options::default()
		};
		let command = compile_command(&app, &dir.join("game/hl2"), &qc, &options, "").unwrap();
		assert_eq!(command.program, dir.join("proton/proton"));
		assert_eq!(
			command.args,
			[
				"runinprefix".to_owned(),
				exe.display().to_string(),
				"-game".into(),
				to_windows_path(&dir.join("game/hl2")),
				"-nop4".into(),
				"-verbose".into(),
				"-nox360".into(),
				"model.qc".into(),
			]
		);
		assert_eq!(command.working_dir, Some(dir.join("src")));
		assert!(matches!(
			compile_command(
				&app,
				&dir.join("game/hl2"),
				&dir.join("src/missing.qc"),
				&options,
				""
			),
			Err(Error::Missing(_))
		));
	}

	#[test]
	fn model_name() {
		let temp = tempfile::tempdir().unwrap();
		let qc = temp.path().join("a.qc");

		write(
			&qc,
			b"// $modelname \"wrong.mdl\"\n$ModelName \"Props\\My Model\\Crate.MDL\"\n$body b \"a.smd\"\n",
		);
		assert_eq!(
			read_model_name(&qc).as_deref(),
			Some("Props/My Model/Crate.MDL")
		);
		write(&qc, b"$cdmaterials x\n\t$modelname props/crate\n");
		assert_eq!(read_model_name(&qc).as_deref(), Some("props/crate.mdl"));
		write(&qc, b"$body b \"a.smd\"\n$modelnames x\n");
		assert_eq!(read_model_name(&qc), None);

		// $modelname in an included .qci, found case-insensitively and through two levels.
		write(&qc, b"$include \"Shared/Common.QCI\"\n$body b \"a.smd\"\n");
		write(
			&temp.path().join("shared/common.qci"),
			b"// $modelname \"no.mdl\"\n$include inner.qci\n",
		);
		write(
			&temp.path().join("shared/inner.qci"),
			b"$modelname \"props\\barrel\"\n",
		);
		assert_eq!(read_model_name(&qc).as_deref(), Some("props/barrel.mdl"));

		// A file including itself must not loop.
		write(&qc, b"$include \"a.qc\"\n");
		assert_eq!(read_model_name(&qc), None);
	}

	#[test]
	fn move_compiled() {
		let temp = tempfile::tempdir().unwrap();
		let game = temp.path().join("game");
		let out = temp.path().join("out");
		for name in [
			"crate.mdl",
			"crate.vvd",
			"crate.dx90.vtx",
			"crate.phy",
			"crate.txt",
			"crate2.mdl",
		] {
			write(&game.join("models/Props/new").join(name), name.as_bytes());
		}
		write(&out.join("models/props/new/crate.mdl"), b"old");

		let result = move_compiled_model(&game, "props/new/crate.mdl", &out);
		assert!(result.ok);
		let target = out.join("models/props/new");
		assert_eq!(
			names(&target),
			["crate.dx90.vtx", "crate.mdl", "crate.phy", "crate.vvd"]
		);
		assert_eq!(
			std::fs::read(target.join("crate.mdl")).unwrap(),
			b"crate.mdl"
		);
		assert_eq!(
			names(&game.join("models/Props/new")),
			["crate.txt", "crate2.mdl"]
		);
		assert_eq!(result.log.len(), 4);

		write(&game.join("models/solo/dir/a.mdl"), b"");
		assert!(move_compiled_model(&game, "solo/dir/a.mdl", &out).ok);
		assert!(!game.join("models/solo").exists());
		assert!(game.join("models").exists());

		write(&game.join("models/top.mdl"), b"");
		assert!(move_compiled_model(&game, "top.mdl", &out).ok);
		assert!(out.join("models/top.mdl").exists());
		assert!(game.join("models").exists());

		let result = move_compiled_model(&game, "missing/x.mdl", &out);
		assert!(!result.ok);
		assert_eq!(result.log.len(), 1);
		let result = move_compiled_model(&game, "props/new/absent.mdl", &out);
		assert!(!result.ok);
		assert_eq!(result.log.len(), 1);
	}
}
