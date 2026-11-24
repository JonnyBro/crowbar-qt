//! What the UI sees of a job: a title, a state, progress and a log.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::watch;

use super::{Event, EventSink};

pub type TaskId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
	Compile,
	View,
	RunGame,
	Hammer,
	Pack,
	Unpack,
	Search,
	WorkshopList,
	Publish,
	Download,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
	Running,
	Succeeded,
	Failed,
	Cancelled,
}

impl TaskState {
	pub fn is_finished(self) -> bool {
		self != Self::Running
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskSnapshot {
	pub id: TaskId,
	pub kind: TaskKind,
	pub title: String,
	pub state: TaskState,
	/// 0.0..=1.0 when known.
	pub progress: Option<f32>,
	/// The file the task works on, e.g. the QC of a compile.
	pub input: Option<PathBuf>,
	/// Files the task made: a compiled model, packed files, a download.
	pub outputs: Vec<PathBuf>,
}

/// Progress updates closer together than this are not sent.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Cancelling one cancels every task that shares it, e.g. a queue of compiles.
pub(super) type CancelSender = Arc<watch::Sender<bool>>;

pub(super) fn new_cancel() -> CancelSender {
	Arc::new(watch::channel(false).0)
}

/// Every running task, for cancelling them by id.
#[derive(Clone)]
pub(super) struct Tasks {
	registry: Arc<Mutex<Registry>>,
	sink: EventSink,
}

#[derive(Default)]
struct Registry {
	next: TaskId,
	running: HashMap<TaskId, CancelSender>,
}

impl Tasks {
	pub fn new(sink: EventSink) -> Self {
		Self {
			registry: Arc::default(),
			sink,
		}
	}

	pub fn start(&self, kind: TaskKind, title: String) -> Task {
		self.start_with(kind, title, new_cancel(), None)
	}

	pub fn start_with(
		&self,
		kind: TaskKind,
		title: String,
		cancel: CancelSender,
		input: Option<PathBuf>,
	) -> Task {
		let id = {
			let mut registry = self.lock();
			registry.next += 1;
			let id = registry.next;
			registry.running.insert(id, cancel.clone());
			id
		};
		let snapshot = TaskSnapshot {
			id,
			kind,
			title,
			state: TaskState::Running,
			progress: None,
			input,
			outputs: Vec::new(),
		};
		self.sink.emit(Event::TaskUpdated(snapshot.clone()));
		Task {
			tasks: self.clone(),
			cancel: cancel.subscribe(),
			inner: Mutex::new(Inner {
				snapshot,
				last_progress: Instant::now(),
			}),
			finished: AtomicBool::new(false),
		}
	}

	pub fn cancel(&self, id: TaskId) {
		if let Some(cancel) = self.lock().running.get(&id) {
			cancel.send_replace(true);
		}
	}

	pub fn cancel_all(&self) {
		for cancel in self.lock().running.values() {
			cancel.send_replace(true);
		}
	}

	fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
		self.registry
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
	}
}

struct Inner {
	snapshot: TaskSnapshot,
	last_progress: Instant,
}

/// A running task. Dropping it without [`Task::finish`] marks it failed, so a
/// job that panics or is aborted doesn't leave a task running forever.
pub(super) struct Task {
	tasks: Tasks,
	cancel: watch::Receiver<bool>,
	inner: Mutex<Inner>,
	finished: AtomicBool,
}

impl Task {
	pub fn id(&self) -> TaskId {
		self.lock().snapshot.id
	}

	pub fn line(&self, line: impl Into<String>) {
		self.tasks.sink.emit(Event::TaskLog {
			id: self.id(),
			line: line.into(),
		});
	}

	/// Sent at most every [`PROGRESS_INTERVAL`], except when it reaches the end.
	pub fn progress(&self, fraction: f32) {
		let mut inner = self.lock();
		let fraction = fraction.clamp(0.0, 1.0);
		inner.snapshot.progress = Some(fraction);
		if fraction >= 1.0 || inner.last_progress.elapsed() >= PROGRESS_INTERVAL {
			inner.last_progress = Instant::now();
			self.tasks
				.sink
				.emit(Event::TaskUpdated(inner.snapshot.clone()));
		}
	}

	pub fn add_output(&self, path: PathBuf) {
		self.lock().snapshot.outputs.push(path);
	}

	pub fn is_cancelled(&self) -> bool {
		*self.cancel.borrow()
	}

	/// Completes when the task is cancelled.
	pub fn cancelled(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
		let mut cancel = self.cancel.clone();
		async move {
			if cancel.wait_for(|cancelled| *cancelled).await.is_err() {
				std::future::pending::<()>().await;
			}
		}
	}

	pub fn finish(&self, state: TaskState) {
		if self.finished.swap(true, Ordering::AcqRel) {
			return;
		}
		let snapshot = {
			let mut inner = self.lock();
			inner.snapshot.state = state;
			if state == TaskState::Succeeded {
				inner.snapshot.progress = Some(1.0);
			}
			inner.snapshot.clone()
		};
		self.tasks.lock().running.remove(&snapshot.id);
		self.tasks.sink.emit(Event::TaskUpdated(snapshot));
	}

	/// Logs `error` and marks the task failed.
	pub fn fail(&self, error: impl std::fmt::Display) {
		self.line(rust_i18n::t!("task.error", error = error));
		self.finish(TaskState::Failed);
	}

	fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
		self.inner
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
	}
}

impl Drop for Task {
	fn drop(&mut self) {
		if !self.finished.load(Ordering::Acquire) {
			self.finish(if self.is_cancelled() {
				TaskState::Cancelled
			} else {
				TaskState::Failed
			});
		}
	}
}
