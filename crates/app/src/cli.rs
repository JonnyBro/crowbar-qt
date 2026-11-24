//! Command-line modes. They print English whatever the interface language is.

use std::fmt::Write as _;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use prybar_core::package::Package;
use prybar_core::steam::{App, Platform};
use prybar_core::{
	AppPaths, Games, Settings, launcher, packer, process, publisher, runner, studiomdl, workshop,
};

#[derive(Parser)]
#[command(name = "prybar", version = prybar_core::VERSION, about = "Source engine modding tool")]
pub struct Cli {
	/// A .qc, .mdl, .vpk, .gma or folder to open in the window.
	pub file: Option<PathBuf>,

	#[command(subcommand)]
	pub command: Option<CliCommand>,
}

#[derive(Subcommand)]
pub enum CliCommand {
	/// Print the detected Source games.
	Games,

	/// Compile a model with the game's studiomdl.
	Compile {
		appid: u32,
		game_dir: PathBuf,
		qc: PathBuf,
		/// Move the compiled files here from the game's models folder.
		output_dir: Option<PathBuf>,
	},

	/// Open HLMV, optionally with a model.
	View {
		appid: u32,
		game_dir: PathBuf,
		mdl: Option<PathBuf>,
	},

	/// Start the game through Steam with -game.
	Run {
		appid: u32,
		game_dir: PathBuf,
		/// Extra launch options.
		#[arg(trailing_var_arg = true, allow_hyphen_values = true)]
		options: Vec<String>,
	},

	/// Open Hammer for a game folder.
	Hammer { appid: u32, game_dir: PathBuf },

	/// Work with VPK and GMA packages.
	#[command(subcommand)]
	Package(PackageCommand),

	/// Garry's Mod workshop, through the game's gmpublish.
	#[command(subcommand)]
	Workshop(WorkshopCommand),

	/// Save workshop items: from Steam's cache when subscribed, otherwise from
	/// their direct link (old items only).
	Download {
		/// Links or IDs of items or collections.
		#[arg(required = true)]
		items: Vec<String>,
		#[arg(short, long)]
		output: PathBuf,
	},
}

#[derive(Subcommand)]
pub enum PackageCommand {
	/// List the files in a package.
	List { file: PathBuf },

	/// Unpack a package without the game's tools.
	Unpack { file: PathBuf, output_dir: PathBuf },

	/// Pack a folder with the game's vpk or gmad.
	Pack {
		appid: u32,
		format: PackFormat,
		folder: PathBuf,
		output: PathBuf,
		/// Split a VPK into numbered parts.
		#[arg(long)]
		multi: bool,
		/// Passed to the packer as they are.
		#[arg(last = true)]
		packer_options: Vec<String>,
	},
}

#[derive(Clone, Copy, ValueEnum)]
pub enum PackFormat {
	Vpk,
	Gma,
}

#[derive(Subcommand)]
pub enum WorkshopCommand {
	/// List your published items.
	List { appid: u32 },

	/// Create an item, or update one with --id.
	Publish {
		appid: u32,
		#[arg(long)]
		id: Option<u64>,
		#[arg(long)]
		addon: Option<PathBuf>,
		/// Any image; it is converted to the 512x512 JPEG gmpublish requires.
		#[arg(long)]
		icon: Option<PathBuf>,
		#[arg(long)]
		changes: Option<String>,
	},
}

pub fn run(command: CliCommand) -> ExitCode {
	let result = match command {
		CliCommand::Games => games(),
		CliCommand::Compile {
			appid,
			game_dir,
			qc,
			output_dir,
		} => compile(appid, &game_dir, &qc, output_dir.as_deref()),
		CliCommand::View { appid, game_dir, mdl } => view(appid, &game_dir, mdl.as_deref()),
		CliCommand::Run {
			appid,
			game_dir,
			options,
		} => run_game(appid, &game_dir, &options),
		CliCommand::Hammer { appid, game_dir } => hammer(appid, &game_dir),
		CliCommand::Package(PackageCommand::List { file }) => list_package(&file),
		CliCommand::Package(PackageCommand::Unpack { file, output_dir }) => {
			unpack(&file, &output_dir)
		}
		CliCommand::Package(PackageCommand::Pack {
			appid,
			format,
			folder,
			output,
			multi,
			packer_options,
		}) => pack(appid, format, &folder, &output, multi, packer_options),
		CliCommand::Workshop(WorkshopCommand::List { appid }) => workshop_list(appid),
		CliCommand::Workshop(WorkshopCommand::Publish {
			appid,
			id,
			addon,
			icon,
			changes,
		}) => publish(
			appid,
			publisher::Request {
				id,
				gma: addon,
				icon,
				changes: changes.unwrap_or_default(),
			},
		),
		CliCommand::Download { items, output } => download(&items, &output),
	};
	match result {
		Ok(code) => code,
		Err(e) => {
			eprintln!("prybar: {e}");
			ExitCode::FAILURE
		}
	}
}

type CliResult = Result<ExitCode, String>;

struct Context {
	settings: Settings,
	games: Games,
}

impl Context {
	/// The same games the window shows: mods, added folders and chosen tools folders included.
	fn load() -> Result<Self, String> {
		let paths = AppPaths::detect().map_err(|e| e.to_string())?;
		let settings = Settings::load(&paths).map_err(|e| e.to_string())?;
		let games = Games::scan(&settings, &paths);
		Ok(Self { settings, games })
	}

	fn app(&self, appid: u32) -> Result<&App, String> {
		self.games
			.app(appid)
			.ok_or_else(|| format!("App {appid} is not installed."))
	}
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
	tokio::runtime::Builder::new_current_thread()
		.enable_all()
		.build()
		.map_err(|e| e.to_string())
}

/// Runs a tool to the end, printing its output; Ctrl+C stops it with everything it started.
fn run_tool(command: &launcher::Command) -> CliResult {
	run_tool_with(command, |line| println!("{line}"))
}

fn run_tool_with(command: &launcher::Command, on_line: impl FnMut(String)) -> CliResult {
	let exit = runtime()?
		.block_on(process::run(command, on_line, async {
			let _ = tokio::signal::ctrl_c().await;
		}))
		.map_err(|e| format!("{}: {e}", command.program.display()))?;
	Ok(match exit.code {
		_ if exit.success => ExitCode::SUCCESS,
		Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1).max(1)),
		None => ExitCode::FAILURE,
	})
}

fn view(appid: u32, game_dir: &Path, mdl: Option<&Path>) -> CliResult {
	let context = Context::load()?;
	let app = context.app(appid)?;
	let command = runner::hlmv_command(app, game_dir, mdl, &context.settings.tool_environment)
		.map_err(|e| e.to_string())?;
	run_tool(&command)
}

fn run_game(appid: u32, game_dir: &Path, options: &[String]) -> CliResult {
	let context = Context::load()?;
	let app = context.app(appid)?;
	let command = runner::game_command(app, Some(game_dir), options).map_err(|e| e.to_string())?;
	run_tool(&command)
}

fn hammer(appid: u32, game_dir: &Path) -> CliResult {
	let context = Context::load()?;
	let app = context.app(appid)?;
	let command = runner::hammer_command(app, game_dir, &context.settings.tool_environment)
		.map_err(|e| e.to_string())?;
	run_tool(&command)
}

fn compile(appid: u32, game_dir: &Path, qc: &Path, output_dir: Option<&Path>) -> CliResult {
	let context = Context::load()?;
	let app = context.app(appid)?;
	let command = studiomdl::compile_command(
		app,
		game_dir,
		qc,
		&studiomdl::Options::default(),
		&context.settings.tool_environment,
	)
	.map_err(|e| e.to_string())?;
	let code = run_tool(&command)?;
	let Some(output_dir) = output_dir else {
		return Ok(code);
	};
	if code != ExitCode::SUCCESS {
		return Ok(code);
	}
	let Some(model_name) = studiomdl::read_model_name(qc) else {
		println!("{}", rust_i18n::t!("studio.no_modelname"));
		return Ok(ExitCode::FAILURE);
	};
	let moved = studiomdl::move_compiled_model(game_dir, &model_name, output_dir);
	for line in &moved.log {
		println!("{line}");
	}
	Ok(if moved.ok {
		ExitCode::SUCCESS
	} else {
		ExitCode::FAILURE
	})
}

fn pack(
	appid: u32,
	format: PackFormat,
	folder: &Path,
	output: &Path,
	multi: bool,
	packer_options: Vec<String>,
) -> CliResult {
	let context = Context::load()?;
	let app = context.app(appid)?;
	let format = match format {
		PackFormat::Vpk => packer::Format::Vpk,
		PackFormat::Gma => packer::Format::Gma,
	};
	let output = std::path::absolute(output).map_err(|e| e.to_string())?;
	let (warn_invalid, extra): (Vec<String>, Vec<String>) =
		packer_options.into_iter().partition(|o| o == "-warninvalid");
	let options = packer::Options {
		multi_chunk: multi,
		warn_invalid: !warn_invalid.is_empty(),
		extra,
	};
	let started = std::time::SystemTime::now();
	let command = packer::pack_command(
		app,
		format,
		folder,
		&output,
		&options,
		&context.settings.tool_environment,
	)
	.map_err(|e| e.to_string())?;
	let code = run_tool(&command)?;
	if code != ExitCode::SUCCESS {
		return Ok(code);
	}
	let result = packer::finish_pack(format, folder, &output, &options, started);
	for line in &result.log {
		println!("{line}");
	}
	Ok(if result.files.is_empty() {
		ExitCode::FAILURE
	} else {
		ExitCode::SUCCESS
	})
}

fn workshop_list(appid: u32) -> CliResult {
	let context = Context::load()?;
	let app = context.app(appid)?;
	let command = publisher::list_command(app, &context.settings.tool_environment)
		.map_err(|e| e.to_string())?;
	let mut parser = publisher::ListParser::default();
	let code = run_tool_with(&command, |line| parser.add_line(&line))?;
	let mut out = String::new();
	for item in parser.items() {
		let _ = writeln!(out, "{}\t{}\t{}", item.id, item.size, item.title);
	}
	print!("{out}");
	Ok(code)
}

fn publish(appid: u32, mut request: publisher::Request) -> CliResult {
	let context = Context::load()?;
	let app = context.app(appid)?;
	let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
	if let Some(icon) = &request.icon {
		let prepared =
			publisher::prepare_icon(icon, &temp.path().join("icon.jpg")).map_err(|e| e.to_string())?;
		request.icon = Some(prepared);
	}
	let command = publisher::publish_command(app, &request, &context.settings.tool_environment)
		.map_err(|e| e.to_string())?;
	run_tool_with(&command, |line| {
		if let Some(id) = publisher::parse_created_id(&line) {
			println!("{line}\n{}", workshop::item_url(id));
		} else {
			println!("{line}");
		}
	})
}

/// Saves workshop items: from Steam's cache when subscribed, otherwise from
/// their direct link (old items only). Collections are expanded.
fn download(items: &[String], output: &Path) -> CliResult {
	let ids = workshop::parse_item_ids(&items.join(" "));
	if ids.is_empty() {
		return Err(format!("Not a workshop link or ID: {}", items.join(" ")));
	}
	let games = Context::load()?.games;
	let client = workshop::Client::new().map_err(|e| e.to_string())?;
	let temp = tempfile::tempdir().map_err(|e| e.to_string())?;

	runtime()?.block_on(async {
		let mut all = Vec::new();
		for id in ids {
			match client.fetch_collection(id).await {
				Ok((children, true)) => {
					println!("Collection {id}: {} item(s)", children.len());
					all.extend(children);
				}
				_ => all.push(id),
			}
		}
		let details = client.fetch_details(&all).await.map_err(|e| e.to_string())?;

		let mut failed = 0;
		for item in details {
			if let Some(error) = &item.error {
				eprintln!("{error}");
				failed += 1;
				continue;
			}
			println!("{} (app {})", item.title, item.app_id);
			let mut files = workshop::find_cached_files(&games.libraries, item.app_id, item.id);
			if !files.is_empty() {
				println!("From Steam's workshop cache.");
			} else if !item.file_url.is_empty() {
				println!("Downloading {}", item.file_url);
				let file = temp.path().join(format!("{}.download", item.id));
				let result = client
					.download(&item.file_url, &file, |_, _| {}, async {
						let _ = tokio::signal::ctrl_c().await;
					})
					.await;
				if let Err(e) = result {
					eprintln!("{e}");
					failed += 1;
					continue;
				}
				files.push(file);
			} else {
				eprintln!(
					"No direct link and not in Steam's cache; subscribe in Steam or use the window with steamcmd."
				);
				failed += 1;
				continue;
			}

			let base_name = workshop::file_base_name(&item, true);
			let count = files.len();
			for (i, file) in files.iter().enumerate() {
				let name = if count == 1 {
					base_name.clone()
				} else {
					format!("{base_name}_{}", i + 1)
				};
				match workshop::save_file(file, output, &name, true) {
					Ok(saved) => println!("Saved {}", saved.display()),
					Err(e) => {
						eprintln!("{e}");
						failed += 1;
					}
				}
			}
		}
		Ok::<_, String>(if failed > 0 {
			ExitCode::FAILURE
		} else {
			ExitCode::SUCCESS
		})
	})
}

fn games() -> CliResult {
	let games = Context::load()?.games;
	if !games.steam_found {
		eprintln!("prybar: no Steam installation found");
	}
	let mut out = String::new();
	for library in &games.libraries {
		let _ = writeln!(out, "library: {}", library.display());
	}
	for tool in &games.compat_tools {
		let _ = writeln!(out, "compat tool: {} -> {}", tool.name, tool.path.display());
	}
	for app in &games.apps {
		let _ = write!(out, "\n{} ({})", app.name, app.id);
		if app.is_mod {
			let _ = write!(out, " mod of {}", app.base_name.as_deref().unwrap_or("?"));
		}
		let platform = match app.platform {
			Platform::Windows => "windows",
			Platform::Linux => "linux",
		};
		let _ = writeln!(out, "\n  build:   {platform}");
		let _ = writeln!(out, "  install: {}", app.install_path.display());
		let _ = writeln!(
			out,
			"  tools:   {}{}",
			display_or(app.tools_path.as_deref(), "-"),
			if app.tools_overridden { " (custom)" } else { "" }
		);
		if app.proton_path.is_some() || app.platform == Platform::Windows {
			let _ = writeln!(
				out,
				"  proton:  {} [{}]",
				display_or(app.proton_path.as_deref(), "not found"),
				app.compat_tool_name.as_deref().unwrap_or_default()
			);
			let _ = writeln!(
				out,
				"  prefix:  {}",
				display_or(app.compat_data_path.as_deref(), "-")
			);
		}
		for dir in &app.game_dirs {
			let _ = writeln!(out, "  game:    {}", dir.display());
		}
	}
	print!("{out}");
	Ok(ExitCode::SUCCESS)
}

fn list_package(file: &Path) -> CliResult {
	let package = Package::open(file).map_err(|e| e.to_string())?;
	let mut out = String::new();
	for entry in &package.entries {
		let _ = writeln!(out, "{}\t{}", entry.size, entry.path);
	}
	print!("{out}");
	Ok(ExitCode::SUCCESS)
}

fn unpack(file: &Path, output_dir: &Path) -> CliResult {
	let package = Package::open(file).map_err(|e| e.to_string())?;
	let indexes: Vec<usize> = (0..package.entries.len()).collect();
	let summary = package.extract_many(
		&indexes,
		output_dir,
		|_, _| ControlFlow::Continue(()),
		|e| eprintln!("{e}"),
	);
	println!(
		"{}: {} file(s) unpacked, {} failed",
		package.format, summary.extracted, summary.failed
	);
	Ok(if summary.failed > 0 {
		ExitCode::FAILURE
	} else {
		ExitCode::SUCCESS
	})
}

fn display_or(path: Option<&Path>, fallback: &str) -> String {
	path.map_or_else(|| fallback.to_owned(), |p| p.display().to_string())
}

#[cfg(test)]
mod tests {
	use super::*;
	use clap::CommandFactory;

	#[test]
	fn definition_is_valid() {
		Cli::command().debug_assert();
	}

	#[test]
	fn pack_takes_packer_options_after_double_dash() {
		let cli = Cli::try_parse_from([
			"prybar", "package", "pack", "4000", "gma", "addon", "out.gma", "--", "-quiet",
		])
		.unwrap();
		let Some(CliCommand::Package(PackageCommand::Pack { packer_options, .. })) = cli.command
		else {
			panic!("not a pack command");
		};
		assert_eq!(packer_options, ["-quiet"]);
	}

	#[test]
	fn run_keeps_hyphenated_launch_options() {
		let cli = Cli::try_parse_from([
			"prybar",
			"run",
			"4000",
			"garrysmod",
			"-console",
			"+map",
			"gm_construct",
		])
		.unwrap();
		let Some(CliCommand::Run { options, .. }) = cli.command else {
			panic!("not a run command");
		};
		assert_eq!(options, ["-console", "+map", "gm_construct"]);
	}

	#[test]
	fn bare_file_opens_the_window() {
		let cli = Cli::try_parse_from(["prybar", "model.qc"]).unwrap();
		assert!(cli.command.is_none());
		assert_eq!(cli.file, Some(PathBuf::from("model.qc")));
	}
}
