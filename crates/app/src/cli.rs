//! Command-line modes. They print English whatever the interface language is.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

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
	let name = match command {
		CliCommand::Games => "games",
		CliCommand::Compile { .. } => "compile",
		CliCommand::View { .. } => "view",
		CliCommand::Run { .. } => "run",
		CliCommand::Hammer { .. } => "hammer",
		CliCommand::Package(_) => "package",
		CliCommand::Workshop(_) => "workshop",
		CliCommand::Download { .. } => "download",
	};
	eprintln!("prybar: `{name}` is not implemented yet");
	ExitCode::from(2)
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
