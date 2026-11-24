rust_i18n::i18n!("locales", fallback = "en");

mod cli;
mod icons;
mod model;
mod shell;
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
	run_window(args.file);
	ExitCode::SUCCESS
}

fn run_window(file: Option<PathBuf>) {
	let paths = match AppPaths::detect().and_then(|p| p.ensure_exist().map(|()| p)) {
		Ok(paths) => paths,
		Err(e) => fatal(&e.to_string()),
	};
	let _log_guard = init_logging(&paths);
	let settings = Settings::load(&paths).unwrap_or_else(|e| {
		tracing::error!("{e}");
		Settings::default()
	});
	// One global locale for this crate, the core and gpui-component.
	rust_i18n::set_locale(settings.language.locale());

	gpui_kit::application()
		.with_assets(icons::AppAssets)
		.run(move |cx: &mut App| {
			gpui_kit::init(cx);
			theme::install(cx);

			let options = WindowOptions {
				window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
					None,
					size(px(1100.), px(720.)),
					cx,
				))),
				window_min_size: Some(size(px(800.), px(500.))),
				titlebar: Some(TitlebarOptions {
					title: Some("Prybar".into()),
					..Default::default()
				}),
				app_id: Some("prybar".into()),
				..Default::default()
			};
			gpui_kit::open_window(options, cx, move |window, cx| {
				theme::apply(&settings, window, cx);
				cx.new(|cx| shell::Shell::new(paths, settings, file, window, cx))
			})
			.expect("open window");
			cx.on_window_closed(|cx, _| cx.quit()).detach();
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
	// zbus warns on every portal file dialog: the request object is gone
	// before it reads its properties, which the dialog doesn't need.
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
