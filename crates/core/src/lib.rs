//! Everything prybar does without a window: finding games, reading packages,
//! running the game's tools. The app and the command line both sit on top of it.

rust_i18n::i18n!("locales", fallback = "en");

pub mod app_update;
pub mod engine;
pub mod error;
mod fsutil;
pub mod games;
pub mod launcher;
pub mod package;
pub mod packer;
pub mod paths;
pub mod process;
pub mod publisher;
pub mod runner;
pub mod settings;
pub mod steam;
pub mod studiomdl;
pub mod vdf;
pub mod workshop;

pub use engine::{Command, Engine, Event};
pub use error::{Error, Result};
pub use games::{Games, Target};
pub use paths::AppPaths;
pub use settings::Settings;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
