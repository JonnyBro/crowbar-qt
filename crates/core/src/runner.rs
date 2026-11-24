//! Starting the game, Hammer and HLMV.

use std::path::Path;

use crate::launcher::{self, Command, Error, Mode, Result, tool_path};
use crate::paths::clean_path;
use crate::steam::{App, Platform};

/// Asks the Steam client to start the game, so Steam picks Proton or the native
/// build itself. `game_dir` becomes -game.
pub fn game_command(app: &App, game_dir: Option<&Path>, options: &[String]) -> Result<Command> {
	let mut args = vec!["-applaunch".to_owned(), app.id.to_string()];
	if let Some(game_dir) = game_dir {
		args.push("-game".into());
		if app.is_mod {
			args.push(if app.platform == Platform::Windows {
				launcher::to_windows_path(game_dir)
			} else {
				clean_path(game_dir).display().to_string()
			});
		} else {
			// A relative -game works for both builds; an absolute one would need
			// Z:\ form under Proton and a host path natively.
			let relative = game_dir.strip_prefix(&app.install_path).unwrap_or(game_dir);
			args.push(relative.display().to_string());
		}
	}
	args.extend(options.iter().cloned());

	let flatpak = app
		.steam_root
		.as_ref()
		.is_some_and(|root| root.to_string_lossy().contains("/.var/app/com.valvesoftware.Steam/"));
	if flatpak {
		let program = launcher::find_executable("flatpak").ok_or(Error::NoSteam)?;
		let mut full = vec!["run".to_owned(), "com.valvesoftware.Steam".to_owned()];
		full.extend(args);
		Ok(Command::new(program, full))
	} else {
		let program = launcher::find_executable("steam").ok_or(Error::NoSteam)?;
		Ok(Command::new(program, args))
	}
}

pub fn hammer_command(app: &App, game_dir: &Path, user_env: &str) -> Result<Command> {
	let executable = launcher::require_tool(app, "hammer")?;
	let windows = launcher::is_windows_executable(&executable);
	let args = vec!["-nop4".into(), "-game".into(), tool_path(windows, game_dir)];
	let mut command = launcher::build(app, &executable, args, Mode::Gui, user_env)?;
	command.working_dir = executable.parent().map(Path::to_path_buf);
	Ok(command)
}

/// HLMV, with a model or without one.
pub fn hlmv_command(app: &App, game_dir: &Path, mdl: Option<&Path>, user_env: &str) -> Result<Command> {
	let executable = launcher::require_tool(app, "hlmv")?;
	if let Some(mdl) = mdl
		&& !mdl.is_file()
	{
		return Err(Error::Missing(mdl.to_path_buf()));
	}
	let windows = launcher::is_windows_executable(&executable);
	// -olddialogs makes HLMV accept a model from anywhere on disk, not only from the game's folders.
	let mut args = vec!["-olddialogs".into(), "-game".into(), tool_path(windows, game_dir)];
	if let Some(mdl) = mdl {
		args.push(tool_path(windows, mdl));
	}
	let mut command = launcher::build(app, &executable, args, Mode::Gui, user_env)?;
	command.working_dir = executable.parent().map(Path::to_path_buf);
	Ok(command)
}

#[cfg(test)]
mod tests {
	use super::*;
	use launcher::to_windows_path;

	fn write(path: &Path, content: &[u8]) {
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		std::fs::write(path, content).unwrap();
	}

	fn windows_app(dir: &Path) -> App {
		App {
			id: 4000,
			name: "Test".into(),
			install_path: dir.join("game"),
			steam_root: Some(dir.join("steam")),
			tools_path: Some(dir.join("game/bin")),
			proton_path: Some(dir.join("proton")),
			compat_data_path: Some(dir.join("compatdata")),
			..App::default()
		}
	}

	#[test]
	fn hlmv() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path();
		write(&dir.join("game/bin/hlmv.exe"), b"MZ");
		write(&dir.join("models/a.mdl"), b"");
		let app = windows_app(dir);

		let mdl = dir.join("models/a.mdl");
		let command = hlmv_command(&app, &dir.join("game/hl2"), Some(&mdl), "").unwrap();
		assert_eq!(
			command.args,
			[
				"run".to_owned(),
				dir.join("game/bin/hlmv.exe").display().to_string(),
				"-olddialogs".into(),
				"-game".into(),
				to_windows_path(&dir.join("game/hl2")),
				to_windows_path(&mdl),
			]
		);
		assert_eq!(command.working_dir, Some(dir.join("game/bin")));

		let without = hlmv_command(&app, &dir.join("game/hl2"), None, "").unwrap();
		assert_eq!(without.args.len(), 5);
		assert!(matches!(
			hlmv_command(&app, &dir.join("game/hl2"), Some(&dir.join("missing.mdl")), ""),
			Err(Error::Missing(_))
		));
	}

	#[test]
	fn hammer_and_game() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path();
		write(&dir.join("game/bin/hammer.exe"), b"MZ");
		let mut app = windows_app(dir);
		let game_dir = dir.join("game/garrysmod");

		let hammer = hammer_command(&app, &game_dir, "").unwrap();
		assert_eq!(
			hammer.args,
			[
				"run".to_owned(),
				dir.join("game/bin/hammer.exe").display().to_string(),
				"-nop4".into(),
				"-game".into(),
				to_windows_path(&game_dir),
			]
		);

		// Only checked where Steam is installed.
		let options = ["-dev".to_owned(), "+map".into(), "gm_construct".into()];
		if let Ok(game) = game_command(&app, Some(&game_dir), &options) {
			assert_eq!(
				game.args,
				["-applaunch", "4000", "-game", "garrysmod", "-dev", "+map", "gm_construct"]
			);
		}

		let modded = App {
			is_mod: true,
			platform: Platform::Windows,
			..app.clone()
		};
		let mod_dir = Path::new("/home/u/Steam/steamapps/sourcemods/mymod");
		if let Ok(game) = game_command(&modded, Some(mod_dir), &[]) {
			assert_eq!(
				game.args,
				[
					"-applaunch",
					"4000",
					"-game",
					"Z:\\home\\u\\Steam\\steamapps\\sourcemods\\mymod"
				]
			);
		}

		app.steam_root = Some("/home/u/.var/app/com.valvesoftware.Steam/.local/share/Steam".into());
		if let Ok(game) = game_command(&app, Some(&game_dir), &[]) {
			assert_eq!(
				game.args[..4],
				["run", "com.valvesoftware.Steam", "-applaunch", "4000"]
			);
		}

		app.tools_path = None;
		assert_eq!(
			hammer_command(&app, &game_dir, ""),
			Err(Error::NoTool("Test".into(), "hammer"))
		);
	}
}
