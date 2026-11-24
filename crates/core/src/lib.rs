//! Everything prybar does without a window: finding games, reading packages,
//! running the game's tools. The app and the command line both sit on top of it.

rust_i18n::i18n!("locales", fallback = "en");

pub mod error;
pub mod paths;
pub mod settings;

pub use error::{Error, Result};
pub use paths::AppPaths;
pub use settings::Settings;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
