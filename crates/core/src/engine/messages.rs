use std::path::PathBuf;
use std::sync::Arc;

use super::task::{TaskId, TaskSnapshot};
use crate::games::{Games, Target};
use crate::inspect::{Inspection, ModelSource};
use crate::package::Package;
use crate::packer;
use crate::publisher::WorkshopItem;
use crate::settings::Settings;
use crate::studiomdl;
use crate::vfs;
use crate::vtf;
use crate::workshop::ItemDetails;

pub const TEXTURE_PREVIEW_SIDE: u32 = 1024;

/// Ties an answer to the question; the UI picks the numbers.
pub type RequestId = u64;

#[derive(Debug, Clone)]
pub enum Command {
	SaveSettings(Settings),
	/// Answered with [`Event::GamesLoaded`].
	RefreshGames,
	/// Compiles the QC files one after another, one task each; cancelling one
	/// stops the rest. With `view_after`, a single successful model opens in HLMV.
	Compile {
		target: Target,
		files: Vec<PathBuf>,
		options: studiomdl::Options,
		output: CompileOutput,
		view_after: bool,
	},
	/// HLMV, with a model or without one.
	View {
		target: Target,
		mdl: Option<PathBuf>,
	},
	/// Asks Steam to start the game.
	RunGame {
		target: Target,
		options: Vec<String>,
	},
	Hammer(Target),
	/// GMA needs no game: it is packed without gmad.
	Pack {
		target: Option<Target>,
		format: packer::Format,
		folder: PathBuf,
		output: PathBuf,
		options: packer::Options,
	},
	/// Answered with [`Event::PackageOpened`].
	OpenPackage {
		request: RequestId,
		path: PathBuf,
	},
	/// Answered with [`Event::EntryRead`]; VTF arrives as a PNG of at most
	/// [`TEXTURE_PREVIEW_SIDE`] pixels a side.
	ReadEntry {
		request: RequestId,
		package: Arc<Package>,
		index: usize,
		max_size: u64,
	},
	Unpack {
		package: Arc<Package>,
		/// Entry indexes; empty unpacks everything.
		indexes: Vec<usize>,
		output_dir: PathBuf,
	},
	/// Searches the game's files; answered with [`Event::SearchResults`].
	SearchPackages {
		request: RequestId,
		target: Target,
		needle: String,
	},
	/// Answered with [`Event::ModelInspected`].
	InspectModel {
		request: RequestId,
		target: Target,
		source: ModelSource,
	},
	/// gmpublish's list of your items, then their details from Steam.
	WorkshopList(Target),
	/// Packs `content` first when it is a folder.
	Publish {
		target: Target,
		id: Option<u64>,
		content: PathBuf,
		icon: Option<PathBuf>,
		changes: String,
	},
	/// Items behind links, IDs and collections in `text`; answered with
	/// [`Event::ItemsFound`].
	LookUpItems {
		request: RequestId,
		text: String,
	},
	/// Queued, one item at a time: from Steam's cache, the direct link or steamcmd.
	Download {
		items: Vec<ItemDetails>,
		output_dir: PathBuf,
		/// Names files "<title> (<id>)" instead of the bare ID.
		with_title: bool,
		/// Unpacks LZMA-compressed downloads.
		convert: bool,
	},
	/// A small download such as a preview image; answered with [`Event::Image`].
	FetchImage {
		request: RequestId,
		url: String,
	},
	/// Answered with [`Event::UpdateAvailable`] when a newer release is out.
	CheckUpdate,
	/// Replaces this binary with the release's; a task, then [`Event::UpdateInstalled`].
	InstallUpdate(crate::app_update::Release),
	Cancel(TaskId),
}

#[derive(Debug, Clone)]
pub enum Event {
	SettingsSaved(Settings),
	SettingsSaveFailed(String),
	GamesLoaded(Arc<Games>),
	TaskUpdated(TaskSnapshot),
	TaskLog {
		id: TaskId,
		line: String,
	},
	PackageOpened {
		request: RequestId,
		result: Result<Arc<Package>, String>,
	},
	EntryRead {
		request: RequestId,
		result: Result<Arc<Vec<u8>>, String>,
		/// Set when `result` is a texture converted to PNG.
		texture: Option<vtf::Info>,
	},
	ModelInspected {
		request: RequestId,
		result: Result<Arc<Inspection>, String>,
	},
	SearchResults {
		request: RequestId,
		task: TaskId,
		/// Sorted by path.
		matches: Vec<vfs::Found>,
	},
	WorkshopItems(Vec<WorkshopItem>),
	/// Details of the items in [`Event::WorkshopItems`], for previews and dates.
	WorkshopDetails(Vec<ItemDetails>),
	/// `gmpublish create` made a new item.
	Published {
		task: TaskId,
		id: u64,
	},
	ItemsFound {
		request: RequestId,
		result: Result<Vec<FoundItem>, String>,
	},
	Image {
		request: RequestId,
		result: Result<Arc<Vec<u8>>, String>,
	},
	UpdateAvailable(crate::app_update::Release),
	UpdateInstalled,
}

/// Where compiled models go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileOutput {
	/// The game's models folder, where studiomdl writes them.
	GameModels,
	/// A folder of this name next to each QC file.
	Subfolder(String),
	Folder(PathBuf),
}

impl CompileOutput {
	/// Where the files of `qc` are moved; `None` leaves them in the game.
	pub fn dir_for(&self, qc: &std::path::Path) -> Option<PathBuf> {
		match self {
			Self::GameModels => None,
			Self::Subfolder(name) => Some(qc.parent().unwrap_or(qc).join(name)),
			Self::Folder(dir) => Some(dir.clone()),
		}
	}
}

/// An item from [`Command::LookUpItems`], with the collection it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundItem {
	pub details: ItemDetails,
	pub collection: Option<u64>,
}
