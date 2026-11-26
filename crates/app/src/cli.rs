//! Command-line modes. They print English whatever the interface language is.

use std::fmt::Write as _;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use prybar_core::package::Package;
use prybar_core::steam::{App, Platform};
use prybar_core::vfs::{self, Vfs};
use prybar_core::{
	AppPaths, Games, Settings, Target, launcher, packer, process, publisher, runner, studiomdl,
	workshop,
};

#[derive(Parser)]
#[command(name = "prybar", version = prybar_core::VERSION, about = "Source engine modding tool")]
pub struct Cli {
	/// .qc, .mdl, .vpk, .gma files or folders to open in the window. When a
	/// window is already open, they open there.
	pub files: Vec<PathBuf>,

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

	/// Check a model's files, materials and textures as the game finds them.
	/// The model is a file or a game path such as models/props_c17/oildrum001.mdl.
	Inspect {
		appid: u32,
		game_dir: PathBuf,
		model: String,
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

	/// Print the game's search paths, first to last.
	Mounts { appid: u32, game_dir: PathBuf },

	/// Print every copy of a file the game has, the loaded one first.
	Which {
		appid: u32,
		game_dir: PathBuf,
		path: String,
	},

	/// Pack a folder with the game's vpk or gmad.
	Pack {
		appid: u32,
		format: PackFormat,
		folder: PathBuf,
		output: PathBuf,
		/// Split a VPK into numbered parts.
		#[arg(long)]
		multi: bool,
		/// Pack with the game's vpk or gmad instead of Prybar's own writers.
		#[arg(long)]
		game_tool: bool,
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
		CliCommand::View {
			appid,
			game_dir,
			mdl,
		} => view(appid, &game_dir, mdl.as_deref()),
		CliCommand::Run {
			appid,
			game_dir,
			options,
		} => run_game(appid, &game_dir, &options),
		CliCommand::Hammer { appid, game_dir } => hammer(appid, &game_dir),
		CliCommand::Inspect {
			appid,
			game_dir,
			model,
		} => inspect_model(appid, &game_dir, &model),
		CliCommand::Package(PackageCommand::List { file }) => list_package(&file),
		CliCommand::Package(PackageCommand::Unpack { file, output_dir }) => {
			unpack(&file, &output_dir)
		}
		CliCommand::Package(PackageCommand::Mounts { appid, game_dir }) => mounts(appid, &game_dir),
		CliCommand::Package(PackageCommand::Which {
			appid,
			game_dir,
			path,
		}) => which(appid, &game_dir, &path),
		CliCommand::Package(PackageCommand::Pack {
			appid,
			format,
			folder,
			output,
			multi,
			game_tool,
			packer_options,
		}) => pack(
			appid,
			format,
			&folder,
			&output,
			multi,
			game_tool,
			packer_options,
		),
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
	game_tool: bool,
	packer_options: Vec<String>,
) -> CliResult {
	let output = std::path::absolute(output).map_err(|e| e.to_string())?;
	let (warn_invalid, extra): (Vec<String>, Vec<String>) = packer_options
		.into_iter()
		.partition(|o| o == "-warninvalid");
	if !game_tool {
		return match format {
			PackFormat::Gma => pack_gma(folder, &output, !warn_invalid.is_empty()),
			PackFormat::Vpk => pack_vpk(folder, &output, multi, &extra),
		};
	}
	let context = Context::load()?;
	let app = context.app(appid)?;
	let format = match format {
		PackFormat::Vpk => packer::Format::Vpk,
		PackFormat::Gma => packer::Format::Gma,
	};
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

/// Our own writer; the game and vpk are not needed.
fn pack_vpk(folder: &Path, output: &Path, multi: bool, extra: &[String]) -> CliResult {
	let options = prybar_core::vpk_writer::Options::from_args(multi, extra)?;
	let packed = prybar_core::vpk_writer::pack(
		folder,
		output,
		options,
		|name, _, _| println!("\t{name}"),
		|| false,
	)
	.map_err(|e| e.to_string())?;
	for path in &packed.skipped {
		println!("left out, hidden or of a refused type: {path}");
	}
	for path in &packed.outputs {
		println!("Saved {}", path.display());
	}
	println!(
		"{} file(s), {} bytes of data",
		packed.files, packed.data_size
	);
	Ok(ExitCode::SUCCESS)
}

/// Our own writer; the game and gmad are not needed.
fn pack_gma(folder: &Path, output: &Path, skip_invalid: bool) -> CliResult {
	let result = prybar_core::gma_writer::pack(
		folder,
		output,
		skip_invalid,
		|name, _, _| println!("\t{name}"),
		|| false,
	);
	match result {
		Ok(packed) => {
			for file in &packed.skipped {
				println!("left out, not on gmad's whitelist: {file}");
			}
			for file in &packed.duplicates {
				println!("left out, same path as another file but for case: {file}");
			}
			println!(
				"Saved {}: {} file(s), {} bytes",
				output.display(),
				packed.files.len(),
				packed.size
			);
			Ok(ExitCode::SUCCESS)
		}
		Err(prybar_core::gma_writer::Error::InvalidFiles(files)) => {
			for file in &files {
				eprintln!("not on gmad's whitelist: {file}");
			}
			Err(format!(
				"{} file(s) are not on gmad's whitelist; pass -- -warninvalid to leave them out",
				files.len()
			))
		}
		Err(e) => Err(e.to_string()),
	}
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
		let prepared = publisher::prepare_icon(icon, &temp.path().join("icon.jpg"))
			.map_err(|e| e.to_string())?;
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
		let details = client
			.fetch_details(&all)
			.await
			.map_err(|e| e.to_string())?;

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
			if app.tools_overridden {
				" (custom)"
			} else {
				""
			}
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

fn vfs_target(appid: u32, game_dir: &Path) -> Result<(Context, Target), String> {
	let context = Context::load()?;
	let app = context.app(appid)?.clone();
	let game_dir = std::path::absolute(game_dir).map_err(|e| e.to_string())?;
	let game_dir = prybar_core::paths::clean_path(&game_dir);
	let target = context
		.games
		.target(&game_dir)
		.unwrap_or(Target { app, game_dir });
	Ok((context, target))
}

fn mounts(appid: u32, game_dir: &Path) -> CliResult {
	let (context, target) = vfs_target(appid, game_dir)?;
	for mount in vfs::mounts(&context.games, &target) {
		let kind = match mount.kind {
			vfs::MountKind::Folder => "folder",
			vfs::MountKind::Package => "package",
		};
		println!("{kind}\t{}", mount.path.display());
	}
	Ok(ExitCode::SUCCESS)
}

fn which(appid: u32, game_dir: &Path, path: &str) -> CliResult {
	let (context, target) = vfs_target(appid, game_dir)?;
	let mounts = vfs::mounts(&context.games, &target);
	let Some(vfs) = Vfs::build(mounts, |_| ControlFlow::Continue(())) else {
		return Ok(ExitCode::FAILURE);
	};
	for (file, error) in vfs.failures() {
		eprintln!("{}: {error}", file.display());
	}
	let Some(found) = vfs.find(path) else {
		println!("{path}: not found");
		return Ok(ExitCode::FAILURE);
	};
	for source in &found.sources {
		match source.kind {
			vfs::MountKind::Package => println!(
				"{}\t{}:{}",
				source.size,
				source.mount.display(),
				source.path
			),
			vfs::MountKind::Folder => println!("{}\t{}", source.size, source.file().display()),
		}
	}
	Ok(ExitCode::SUCCESS)
}

fn inspect_model(appid: u32, game_dir: &Path, model: &str) -> CliResult {
	use prybar_core::inspect::{self, CompanionState, MaterialProblem, ModelSource};

	let (context, target) = vfs_target(appid, game_dir)?;
	let source = if Path::new(model).is_file() {
		ModelSource::File(std::path::absolute(model).map_err(|e| e.to_string())?)
	} else {
		ModelSource::Game(model.to_owned())
	};
	let mounts = vfs::mounts(&context.games, &target);
	let Some(vfs) = Vfs::build(mounts, |_| ControlFlow::Continue(())) else {
		return Ok(ExitCode::FAILURE);
	};
	let report = inspect::inspect(&vfs, &source)?;
	let m = &report.model;
	let place = |s: &vfs::Source| match s.kind {
		vfs::MountKind::Package => format!("{}:{}", s.mount.display(), s.path),
		vfs::MountKind::Folder => s.file().display().to_string(),
	};

	println!("model:     {}", place(&report.file));
	if let Some(path) = &report.game_path {
		println!("in game:   {path}");
	}
	if let Some(copy) = &report.game_copy {
		println!("! the game loads another copy: {}", place(copy));
	}
	println!(
		"header:    \"{}\", version {}, checksum {:08x}{}",
		m.name,
		m.version,
		m.checksum,
		if m.is_static_prop() {
			", $staticprop"
		} else {
			""
		}
	);
	println!(
		"contents:  {} bones, {} sequences, {} attachments, {} flexes, surfaceprop \"{}\"",
		m.bones.len(),
		m.sequences.len(),
		m.attachments.len(),
		m.flex_count,
		m.surface_prop
	);
	println!();
	for c in &report.companions {
		let line = match &c.state {
			CompanionState::Missing if c.required => "MISSING".to_owned(),
			CompanionState::Missing => "none".to_owned(),
			CompanionState::Found { source, matches } => format!(
				"{}{}",
				place(source),
				if *matches { "" } else { "  CHECKSUM DIFFERS" }
			),
			CompanionState::Unreadable { source, error } => {
				format!("{}  UNREADABLE: {error}", place(source))
			}
		};
		println!(".{:<10}{line}", c.extension);
	}
	for include in &report.includes {
		let place = include
			.found
			.as_ref()
			.map_or("MISSING".to_owned(), |f| place(&f.sources[0]));
		println!("include    {}  {place}", include.path);
	}
	println!();
	println!("$cdmaterials: {:?}", m.material_folders);
	for material in &report.materials {
		match &material.vmt {
			Some(found) => println!(
				"{}  ->  {}  [{}]  {}",
				material.name,
				found.path,
				material.shader.as_deref().unwrap_or("?"),
				place(&found.sources[0])
			),
			None => println!("{}  ->  MISSING, tried {:?}", material.name, material.tried),
		}
		for problem in &material.problems {
			match problem {
				MaterialProblem::Missing => {}
				MaterialProblem::Unreadable(e) => println!("    ! unreadable: {e}"),
				MaterialProblem::Broken(e) => println!("    ! broken: {e}"),
				MaterialProblem::WorldShader(s) => println!("    ! {s} is a shader for brushes"),
				MaterialProblem::IncludeMissing(p) => println!("    ! includes {p}, missing"),
			}
		}
		for texture in &material.textures {
			let place = texture
				.found
				.as_ref()
				.map_or("MISSING".to_owned(), |f| place(&f.sources[0]));
			println!("    {} {}  {place}", texture.param, texture.name);
		}
	}
	if m.skins.len() > 1 {
		println!();
		for (i, row) in m.skins.iter().enumerate() {
			let names: Vec<&str> = row
				.iter()
				.map(|&t| m.materials.get(t as usize).map_or("?", String::as_str))
				.collect();
			println!("skin {i}: {}", names.join(", "));
		}
	}
	println!();
	for part in &m.body_parts {
		let models: Vec<String> = part
			.models
			.iter()
			.map(|s| {
				if s.name.is_empty() {
					"blank".to_owned()
				} else {
					format!("{} ({} vertices)", s.name, s.vertices)
				}
			})
			.collect();
		println!("bodygroup {}: {}", part.name, models.join(", "));
	}
	let problems = report.problem_count();
	println!();
	println!("{problems} problem(s)");
	Ok(if problems == 0 {
		ExitCode::SUCCESS
	} else {
		ExitCode::FAILURE
	})
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
		assert_eq!(cli.files, [PathBuf::from("model.qc")]);

		let cli = Cli::try_parse_from(["prybar", "a.qc", "b.vpk"]).unwrap();
		assert!(cli.command.is_none());
		assert_eq!(cli.files.len(), 2);

		let cli = Cli::try_parse_from(["prybar", "games"]).unwrap();
		assert!(matches!(cli.command, Some(CliCommand::Games)));
		assert!(cli.files.is_empty());
	}
}
