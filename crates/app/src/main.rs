rust_i18n::i18n!("locales", fallback = "en");

mod cli;
mod desktop;
mod icons;
mod model;
mod shell;
mod single_instance;
mod theme;
mod ui_state;
mod views;
mod widgets;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use gpui_kit::*;
use prybar_core::{AppPaths, Settings};
use tracing_subscriber::prelude::*;

fn main() -> ExitCode {
	let args = cli::Cli::parse();
	if let Some(command) = args.command {
		return cli::run(command);
	}
	run_window(args.files);
	ExitCode::SUCCESS
}

fn run_window(files: Vec<PathBuf>) {
	let paths = match AppPaths::detect().and_then(|p| p.ensure_exist().map(|()| p)) {
		Ok(paths) => paths,
		Err(e) => fatal(&e.to_string()),
	};
	let _log_guard = init_logging(&paths);
	let listener = match single_instance::start(&paths, &files) {
		single_instance::Start::Forwarded => return,
		single_instance::Start::First(listener) => listener,
	};
	let _socket_cleanup = single_instance::Cleanup;
	desktop::refresh_if_installed();
	let (request_tx, request_rx) = smol::channel::unbounded::<Vec<PathBuf>>();
	if let Some(listener) = listener {
		single_instance::listen(listener, move |files| {
			let _ = request_tx.try_send(files);
		});
	}
	let settings = Settings::load(&paths).unwrap_or_else(|e| {
		tracing::error!("{e}");
		Settings::default()
	});
	// One global locale for this crate, the core and gpui-component.
	rust_i18n::set_locale(settings.language.locale());

	let saved_window = ui_state::UiState::load(&paths.ui_state_file()).window;

	gpui_kit::application()
		.with_assets(icons::AppAssets)
		.run(move |cx: &mut App| {
			gpui_kit::init(cx);
			theme::install(cx);
			// System notifications are sent under this name.
			cx.set_app_identity("prybar", "Prybar");

			let options = WindowOptions {
				window_bounds: Some(saved_window.map(|b| b.to_window()).unwrap_or_else(|| {
					WindowBounds::Windowed(Bounds::centered(None, size(px(1100.), px(720.)), cx))
				})),
				window_min_size: Some(size(px(800.), px(500.))),
				titlebar: Some(TitlebarOptions {
					title: Some("Prybar".into()),
					..Default::default()
				}),
				app_id: Some("prybar".into()),
				..Default::default()
			};
			let main = gpui_kit::open_window(options, cx, move |window, cx| {
				theme::apply(&settings, window, cx);
				cx.new(|cx| shell::Shell::new(paths, settings, files, request_rx, window, cx))
			})
			.expect("open window");
			cx.on_system_notification_response(move |_, cx| {
				let _ = main.0.update(cx, |_, window, _| window.activate_window());
			});
			// Viewer windows can close on their own; closing the main one ends the app.
			let main_id = main.0.window_id();
			cx.on_window_closed(move |cx, closed| {
				if closed == main_id {
					cx.quit();
				}
			})
			.detach();
			cx.activate(true);
		});
}

fn fatal(message: &str) -> ! {
	eprintln!("fatal: {message}");
	rfd::MessageDialog::new()
		.set_level(rfd::MessageLevel::Error)
		.set_title("Prybar")
		.set_description(message)
		.show();
	std::process::exit(1);
}

/// Logs go to stderr and to a daily file in the logs folder.
fn init_logging(paths: &AppPaths) -> Option<tracing_appender::non_blocking::WorkerGuard> {
	use tracing_subscriber::{EnvFilter, fmt};
	// zbus warns on every portal file dialog; the warning is harmless.
	let default = if cfg!(debug_assertions) {
		"info,prybar=debug,prybar_core=debug,wgpu_hal=warn,wgpu_core=warn,zbus=error"
	} else {
		"info,wgpu_hal=warn,wgpu_core=warn,zbus=error"
	};
	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
	// tracing writes timestamps in UTC by default; these use the local time zone.
	let timer = fmt::time::ChronoLocal::rfc_3339();
	let stderr = fmt::layer()
		.with_timer(timer.clone())
		.with_writer(std::io::stderr);
	let file = tracing_appender::rolling::Builder::new()
		.rotation(tracing_appender::rolling::Rotation::DAILY)
		.filename_prefix("prybar")
		.filename_suffix("log")
		.max_log_files(7)
		.build(paths.logs_dir());
	match file {
		Ok(appender) => {
			let (writer, guard) = tracing_appender::non_blocking(appender);
			let file = fmt::layer()
				.with_timer(timer)
				.with_ansi(false)
				.with_writer(writer);
			tracing_subscriber::registry()
				.with(filter)
				.with(stderr)
				.with(file)
				.init();
			Some(guard)
		}
		Err(e) => {
			tracing_subscriber::registry()
				.with(filter)
				.with(stderr)
				.init();
			tracing::warn!("file logging disabled: {e}");
			None
		}
	}
}

/// rust_i18n shows a key missing from this crate's file as the bare key.
#[cfg(test)]
mod locale_keys {
	use std::path::Path;

	fn used_keys(dir: &Path, out: &mut Vec<String>) {
		for entry in std::fs::read_dir(dir).unwrap().flatten() {
			let path = entry.path();
			if path.is_dir() {
				used_keys(&path, out);
			} else if path.extension().is_some_and(|e| e == "rs") {
				let text = std::fs::read_to_string(&path).unwrap();
				for (at, _) in text.match_indices("t!(") {
					let before = text[..at].chars().next_back();
					if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
						continue;
					}
					let rest = text[at + 3..].trim_start();
					let Some(rest) = rest.strip_prefix('"') else {
						continue;
					};
					let key = &rest[..rest.find('"').unwrap()];
					if key
						.chars()
						.all(|c| c.is_ascii_alphanumeric() || "._".contains(c))
					{
						out.push(key.to_owned());
					}
				}
			}
		}
	}

	#[test]
	fn every_key_is_in_this_crates_file() {
		let root = Path::new(env!("CARGO_MANIFEST_DIR"));
		let yml = std::fs::read_dir(root.join("locales"))
			.unwrap()
			.flatten()
			.map(|e| std::fs::read_to_string(e.path()).unwrap())
			.collect::<String>();
		let defined: Vec<&str> = yml
			.lines()
			.filter_map(|line| line.strip_suffix(':'))
			.filter(|key| !key.starts_with(' '))
			.collect();
		let mut used = Vec::new();
		used_keys(&root.join("src"), &mut used);
		used.sort();
		used.dedup();
		let missing: Vec<&String> = used
			.iter()
			.filter(|k| !defined.contains(&k.as_str()))
			.collect();
		assert!(missing.is_empty(), "{missing:?}");
	}
}
