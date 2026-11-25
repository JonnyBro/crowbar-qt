//! Background side of the app: the UI sends [`Command`]s, the engine works on a
//! tokio runtime in its own thread and reports back with [`Event`]s.

mod messages;
mod packages;
mod task;
mod tools;
mod update;
mod workshop;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;

use tokio::sync::{Mutex, mpsc};

use crate::games::Games;
use crate::paths::AppPaths;
use crate::settings::Settings;

pub use messages::{Command, CompileOutput, Event, FoundItem, RequestId};
pub use task::{TaskId, TaskKind, TaskSnapshot, TaskState};

/// Called after every event so the UI can wake up and repaint.
pub type Notifier = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub(crate) struct EventSink {
	tx: std_mpsc::Sender<Event>,
	notify: Notifier,
}

impl EventSink {
	pub(crate) fn emit(&self, event: Event) {
		if self.tx.send(event).is_ok() {
			(self.notify)();
		}
	}
}

pub struct Engine {
	commands: Option<mpsc::UnboundedSender<Command>>,
	events: std_mpsc::Receiver<Event>,
	thread: Option<JoinHandle<()>>,
}

impl Engine {
	pub fn start(paths: AppPaths, settings: Settings, notify: Notifier) -> std::io::Result<Self> {
		let runtime = tokio::runtime::Builder::new_multi_thread()
			.enable_all()
			.thread_name("engine-worker")
			.build()?;
		let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
		let (event_tx, event_rx) = std_mpsc::channel();
		let sink = EventSink {
			tx: event_tx,
			notify,
		};
		let thread = std::thread::Builder::new()
			.name("engine".into())
			.spawn(move || {
				runtime.block_on(run(paths, settings, cmd_rx, sink));
				// Jobs still running (a tool waiting for its children) must not
				// keep the app from quitting.
				runtime.shutdown_timeout(std::time::Duration::from_secs(3));
			})?;
		Ok(Self {
			commands: Some(cmd_tx),
			events: event_rx,
			thread: Some(thread),
		})
	}

	pub fn send(&self, command: Command) {
		if let Some(tx) = &self.commands
			&& tx.send(command).is_err()
		{
			tracing::error!("engine is not running, command dropped");
		}
	}

	pub fn poll_events(&self) -> impl Iterator<Item = Event> + '_ {
		self.events.try_iter()
	}
}

impl Drop for Engine {
	fn drop(&mut self) {
		// Closing the channel ends the command loop, which lets the thread exit.
		self.commands.take();
		if let Some(thread) = self.thread.take()
			&& thread.join().is_err()
		{
			tracing::error!("engine thread panicked");
		}
	}
}

/// What every job may need; cheap to clone into one.
#[derive(Clone)]
pub(crate) struct Context {
	paths: AppPaths,
	settings: Arc<Settings>,
	games: Arc<Games>,
	sink: EventSink,
	tasks: task::Tasks,
	scratch: Arc<Scratch>,
	/// Downloads run one at a time: steamcmd allows one run, and Steam's servers prefer it.
	download_lock: Arc<Mutex<()>>,
}

impl Context {
	fn user_env(&self) -> &str {
		&self.settings.tool_environment
	}

	/// A folder under this run's scratch folder.
	fn scratch_dir(&self, name: &str) -> std::io::Result<PathBuf> {
		let dir = self.scratch.0.join(name);
		std::fs::create_dir_all(&dir)?;
		Ok(dir)
	}
}

/// This run's temporary folder, removed when the engine stops. Folders left by
/// runs that were killed are removed at start.
struct Scratch(PathBuf);

impl Scratch {
	fn create(paths: &AppPaths) -> Self {
		let root = paths.cache_dir().join("scratch");
		if let Ok(read) = std::fs::read_dir(&root) {
			for entry in read.flatten() {
				let stale = entry
					.file_name()
					.to_str()
					.and_then(|name| name.parse::<u32>().ok())
					.is_some_and(|pid| !Path::new(&format!("/proc/{pid}")).exists());
				if stale {
					let _ = std::fs::remove_dir_all(entry.path());
				}
			}
		}
		Self(root.join(std::process::id().to_string()))
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		if let Err(e) = std::fs::remove_dir_all(&self.0)
			&& e.kind() != std::io::ErrorKind::NotFound
		{
			tracing::warn!("could not remove {}: {e}", self.0.display());
		}
	}
}

async fn run(
	paths: AppPaths,
	settings: Settings,
	mut commands: mpsc::UnboundedReceiver<Command>,
	sink: EventSink,
) {
	tracing::info!("engine started");
	let mut context = Context {
		scratch: Arc::new(Scratch::create(&paths)),
		paths,
		settings: Arc::new(settings),
		games: Arc::default(),
		tasks: task::Tasks::new(sink.clone()),
		sink,
		download_lock: Arc::default(),
	};
	let (games_tx, mut games_rx) = mpsc::unbounded_channel::<Arc<Games>>();

	loop {
		let command = tokio::select! {
			command = commands.recv() => match command {
				Some(command) => command,
				None => break,
			},
			Some(games) = games_rx.recv() => {
				context.games = games.clone();
				context.sink.emit(Event::GamesLoaded(games));
				continue;
			}
		};
		match command {
			Command::SaveSettings(settings) => save_settings(&mut context, settings).await,
			Command::RefreshGames => {
				let settings = context.settings.clone();
				let paths = context.paths.clone();
				let tx = games_tx.clone();
				tokio::task::spawn_blocking(move || {
					let _ = tx.send(Arc::new(Games::scan(&settings, &paths)));
				});
			}
			Command::Cancel(id) => context.tasks.cancel(id),
			Command::Compile {
				target,
				files,
				options,
				output,
				view_after,
			} => {
				tokio::spawn(tools::compile(
					context.clone(),
					target,
					files,
					options,
					output,
					view_after,
				));
			}
			Command::View { target, mdl } => {
				tokio::spawn(tools::view(context.clone(), target, mdl));
			}
			Command::RunGame { target, options } => {
				tokio::spawn(tools::run_game(context.clone(), target, options));
			}
			Command::Hammer(target) => {
				tokio::spawn(tools::hammer(context.clone(), target));
			}
			Command::Pack {
				target,
				format,
				folder,
				output,
				options,
			} => {
				tokio::spawn(tools::pack_job(
					context.clone(),
					target,
					format,
					folder,
					output,
					options,
				));
			}
			Command::OpenPackage { request, path } => packages::open(&context, request, path),
			Command::ReadEntry {
				request,
				package,
				index,
				max_size,
			} => packages::read_entry(&context, request, package, index, max_size),
			Command::Unpack {
				package,
				indexes,
				output_dir,
			} => packages::unpack(&context, package, indexes, output_dir),
			Command::SearchPackages {
				request,
				target,
				needle,
			} => {
				packages::search(&context, request, target, &needle);
			}
			Command::WorkshopList(target) => {
				tokio::spawn(workshop::list(context.clone(), target));
			}
			Command::Publish {
				target,
				id,
				content,
				icon,
				changes,
			} => {
				tokio::spawn(workshop::publish(
					context.clone(),
					target,
					id,
					content,
					icon,
					changes,
				));
			}
			Command::LookUpItems { request, text } => {
				tokio::spawn(workshop::look_up(context.clone(), request, text));
			}
			Command::Download {
				items,
				output_dir,
				with_title,
				convert,
			} => {
				tokio::spawn(workshop::download(
					context.clone(),
					items,
					output_dir,
					with_title,
					convert,
				));
			}
			Command::FetchImage { request, url } => {
				tokio::spawn(workshop::fetch_image(context.clone(), request, url));
			}
			Command::CheckUpdate => {
				tokio::spawn(update::check(context.clone()));
			}
			Command::InstallUpdate(release) => {
				tokio::spawn(update::install(context.clone(), release));
			}
		}
	}
	context.tasks.cancel_all();
	tracing::info!("engine stopped");
}

async fn save_settings(context: &mut Context, settings: Settings) {
	let paths = context.paths.clone();
	let to_save = settings.clone();
	let saved = tokio::task::spawn_blocking(move || to_save.save(&paths)).await;
	match saved {
		Ok(Ok(())) => {
			context.settings = Arc::new(settings.clone());
			context.sink.emit(Event::SettingsSaved(settings));
		}
		Ok(Err(e)) => context.sink.emit(Event::SettingsSaveFailed(e.to_string())),
		Err(e) => context.sink.emit(Event::SettingsSaveFailed(e.to_string())),
	}
}

#[cfg(test)]
mod tests;
