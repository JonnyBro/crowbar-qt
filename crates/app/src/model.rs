//! What every page shares: the engine, the games, the game being worked on,
//! the settings and the tasks with their logs.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use prybar_core::engine::{RequestId, TaskId, TaskSnapshot, TaskState};
use prybar_core::{AppPaths, Command, Engine, Event, Games, Settings, Target, launcher};

use crate::ui_state::UiState;

/// A task's log keeps at most this many lines; studiomdl -verbose can print far more.
const MAX_LOG_LINES: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
	Normal,
	Warning,
	Error,
}

/// A path in a log line that exists on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
	/// Byte range of the path (and its "(line)" suffix) in the line.
	pub range: std::ops::Range<usize>,
	pub path: PathBuf,
	pub line: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LogLine {
	pub text: SharedString,
	pub severity: Severity,
	pub link: Option<Link>,
}

impl LogLine {
	pub fn new(text: String) -> Self {
		Self {
			severity: severity_of(&text),
			link: find_link(&text),
			text: text.into(),
		}
	}
}

fn severity_of(line: &str) -> Severity {
	let text = line.trim_start().to_uppercase();
	if ["ERROR", "ОШИБКА", "FATAL"]
		.iter()
		.any(|p| text.starts_with(p))
	{
		Severity::Error
	} else if ["WARNING", "ВНИМАНИЕ"].iter().any(|p| text.starts_with(p)) {
		Severity::Warning
	} else {
		Severity::Normal
	}
}

/// The first absolute path in `line` that exists. A quoted path may contain
/// spaces; a bare one ends at whitespace or punctuation. "(12)" right after a
/// path is a line number, as studiomdl prints it.
pub fn find_link(line: &str) -> Option<Link> {
	let bytes = line.as_bytes();
	for (start, _) in line.match_indices('/') {
		let prev = start.checked_sub(1).map(|i| bytes[i]);
		let quoted = prev == Some(b'"');
		let at_boundary = prev.is_none_or(|b| {
			b.is_ascii_whitespace() || matches!(b, b'"' | b'\'' | b'(' | b'=' | b'[')
		}) || line[..start].ends_with('«');
		if !at_boundary {
			continue;
		}
		let rest = &line[start..];
		let len = if quoted {
			match rest.find('"') {
				Some(len) => len,
				None => continue,
			}
		} else {
			rest.find(|c: char| {
				c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '|' | ',' | ':' | '»')
			})
			.unwrap_or(rest.len())
		};
		let mut path = &rest[..len];
		let mut end = start + len;
		let mut line_number = None;
		if let Some(open) = path.rfind('(')
			&& let Some(digits) = path[open + 1..].strip_suffix(')')
			&& !digits.is_empty()
			&& digits.bytes().all(|b| b.is_ascii_digit())
		{
			line_number = digits.parse().ok();
			path = &path[..open];
		} else {
			let trimmed = path.trim_end_matches(['.', ')']);
			end -= path.len() - trimmed.len();
			path = trimmed;
		}
		if path.len() < 2 {
			continue;
		}
		if let Some(real) = launcher::find_path_ignoring_case(Path::new("/"), path) {
			return Some(Link {
				range: start..end,
				path: real,
				line: line_number,
			});
		}
	}
	None
}

pub struct TaskView {
	pub snapshot: TaskSnapshot,
	pub log: Vec<LogLine>,
	started: Instant,
}

/// An engine event that only the pages know what to do with.
pub struct EngineEvent(pub Event);

pub struct TaskFinished {
	pub snapshot: TaskSnapshot,
	pub took: Duration,
}

pub struct Model {
	pub engine: Rc<Engine>,
	pub paths: AppPaths,
	pub settings: Settings,
	pub games: Arc<Games>,
	pub games_loaded: bool,
	pub target: Option<Target>,
	pub tasks: Vec<TaskView>,
	pub ui: UiState,
	next_request: RequestId,
}

impl EventEmitter<EngineEvent> for Model {}
impl EventEmitter<TaskFinished> for Model {}

impl Model {
	pub fn new(engine: Rc<Engine>, paths: AppPaths, settings: Settings) -> Self {
		let ui = UiState::load(&paths.ui_state_file());
		Self {
			engine,
			paths,
			settings,
			games: Arc::default(),
			games_loaded: false,
			target: None,
			tasks: Vec::new(),
			ui,
			next_request: 0,
		}
	}

	pub fn send(&self, command: Command) {
		self.engine.send(command);
	}

	pub fn request_id(&mut self) -> RequestId {
		self.next_request += 1;
		self.next_request
	}

	pub fn save_ui(&self) {
		self.ui.save(&self.paths.ui_state_file());
	}

	/// Keeps the change here at once and saves it in the background.
	pub fn update_settings(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
		change(&mut self.settings);
		self.send(Command::SaveSettings(self.settings.clone()));
		cx.notify();
	}

	/// Saves the change, then scans the games again with it.
	pub fn update_game_settings(
		&mut self,
		change: impl FnOnce(&mut Settings),
		cx: &mut Context<Self>,
	) {
		self.update_settings(change, cx);
		self.send(Command::RefreshGames);
	}

	pub fn set_target(&mut self, target: Option<Target>, cx: &mut Context<Self>) {
		let dir = target.as_ref().map(|t| t.game_dir.clone());
		self.target = target;
		if self.settings.current_game != dir {
			self.update_settings(|s| s.current_game = dir, cx);
		}
		cx.notify();
	}

	pub fn task(&self, id: TaskId) -> Option<&TaskView> {
		self.tasks.iter().find(|t| t.snapshot.id == id)
	}

	/// The newest task that worked on `input`.
	pub fn task_for(&self, input: &Path) -> Option<&TaskView> {
		self.tasks
			.iter()
			.rev()
			.find(|t| t.snapshot.input.as_deref() == Some(input))
	}

	pub fn running(&self) -> usize {
		self.tasks
			.iter()
			.filter(|t| t.snapshot.state == TaskState::Running)
			.count()
	}

	pub fn clear_finished(&mut self, cx: &mut Context<Self>) {
		self.tasks.retain(|t| !t.snapshot.state.is_finished());
		cx.notify();
	}

	/// Takes in what the engine reports; what the pages need is passed on.
	pub fn handle(&mut self, event: Event, cx: &mut Context<Self>) {
		match event {
			Event::GamesLoaded(games) => {
				self.games_loaded = true;
				let saved = self
					.target
					.as_ref()
					.map(|t| t.game_dir.clone())
					.or_else(|| self.settings.current_game.clone());
				self.target = games.current(saved.as_deref());
				self.games = games;
				cx.notify();
			}
			Event::TaskUpdated(snapshot) => {
				match self.tasks.iter_mut().find(|t| t.snapshot.id == snapshot.id) {
					Some(task) => {
						let finished =
							!task.snapshot.state.is_finished() && snapshot.state.is_finished();
						task.snapshot = snapshot;
						if finished {
							cx.emit(TaskFinished {
								snapshot: task.snapshot.clone(),
								took: task.started.elapsed(),
							});
						}
					}
					None => self.tasks.push(TaskView {
						snapshot,
						log: Vec::new(),
						started: Instant::now(),
					}),
				}
				cx.notify();
			}
			Event::TaskLog { id, line } => {
				if let Some(task) = self.tasks.iter_mut().find(|t| t.snapshot.id == id)
					&& task.log.len() < MAX_LOG_LINES
				{
					task.log.push(LogLine::new(line));
				}
				cx.notify();
			}
			Event::SettingsSaved(settings) => {
				self.settings = settings;
				cx.emit(EngineEvent(Event::SettingsSaved(self.settings.clone())));
				cx.notify();
			}
			other => cx.emit(EngineEvent(other)),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	// gpui_kit's prelude has a `test` attribute of its own.
	use ::core::prelude::v1::test;

	#[test]
	fn severities() {
		assert_eq!(severity_of("ERROR: x"), Severity::Error);
		assert_eq!(severity_of("  ошибка: x"), Severity::Error);
		assert_eq!(severity_of("WARNING: x"), Severity::Warning);
		assert_eq!(severity_of("Moved"), Severity::Normal);
	}

	#[test]
	fn links() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path().canonicalize().unwrap();
		std::fs::create_dir_all(dir.join("my addon")).unwrap();
		std::fs::write(dir.join("my addon/a.qc"), "").unwrap();
		let d = dir.display();

		let line = format!("Error at {d}/my addon/a.qc(12): bad");
		assert_eq!(find_link(&line), None, "a bare path ends at the space");

		let line = format!("writing \"{d}/my addon/a.qc\"");
		let link = find_link(&line).unwrap();
		assert_eq!(link.path, dir.join("my addon/a.qc"));
		assert_eq!(&line[link.range], format!("{d}/my addon/a.qc"));

		std::fs::write(dir.join("b.smd"), "").unwrap();
		let line = format!("ERROR: {d}/B.SMD(3) : unknown token.");
		let link = find_link(&line).unwrap();
		assert_eq!(link.path, dir.join("b.smd"));
		assert_eq!(link.line, Some(3));
		assert_eq!(&line[link.range], format!("{d}/B.SMD(3)"));

		let line = format!("Unpacking 1 file(s) to «{d}/b.smd»…");
		let link = find_link(&line).unwrap();
		assert_eq!(&line[link.range], format!("{d}/b.smd"));

		let line = format!("Saved {d}/b.smd.");
		let link = find_link(&line).unwrap();
		assert_eq!(&line[link.range], format!("{d}/b.smd"));

		assert_eq!(find_link("a/b and /nonexistent/x"), None);
	}
}
