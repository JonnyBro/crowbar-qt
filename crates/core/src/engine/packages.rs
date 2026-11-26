//! Reading, unpacking and searching packages, off the async threads.

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;

use rust_i18n::t;

use super::Context;
use super::messages::{Event, RequestId, TEXTURE_PREVIEW_SIDE};
use super::task::{TaskKind, TaskState};
use crate::decompile;
use crate::games::Target;
use crate::inspect::{self, ModelSource};
use crate::package::Package;
use crate::vfs::{self, Vfs};
use crate::vtf;

/// More would only bury what the user is after.
const MAX_SEARCH_RESULTS: usize = 5000;

pub(super) fn open(context: &Context, request: RequestId, path: PathBuf) {
	let sink = context.sink.clone();
	tokio::task::spawn_blocking(move || {
		let result = Package::open(&path)
			.map(Arc::new)
			.map_err(|e| e.to_string());
		sink.emit(Event::PackageOpened { request, result });
	});
}

pub(super) fn read_entry(
	context: &Context,
	request: RequestId,
	package: Arc<Package>,
	index: usize,
	max_size: u64,
) {
	let sink = context.sink.clone();
	tokio::task::spawn_blocking(move || {
		let Some(entry) = package.entries.get(index) else {
			let result = Err(t!("task.no_entry").into_owned());
			sink.emit(Event::EntryRead {
				request,
				result,
				texture: None,
			});
			return;
		};
		let data = package.read(entry, max_size).map_err(|e| e.to_string());
		let event = if entry.path.to_ascii_lowercase().ends_with(".vtf") {
			match data.and_then(|data| {
				vtf::decode(&data, TEXTURE_PREVIEW_SIDE).map_err(|e| e.to_string())
			}) {
				Ok((info, texture)) => Event::EntryRead {
					request,
					result: Ok(Arc::new(texture.to_png())),
					texture: Some(info),
				},
				Err(e) => Event::EntryRead {
					request,
					result: Err(e),
					texture: None,
				},
			}
		} else {
			Event::EntryRead {
				request,
				result: data.map(Arc::new),
				texture: None,
			}
		};
		sink.emit(event);
	});
}

pub(super) fn unpack(
	context: &Context,
	package: Arc<Package>,
	indexes: Vec<usize>,
	output_dir: PathBuf,
) {
	let name = package
		.file
		.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_default();
	let task = context.tasks.start(
		TaskKind::Unpack,
		t!("task.unpack", name = name).into_owned(),
	);
	let indexes = if indexes.is_empty() {
		(0..package.entries.len()).collect()
	} else {
		indexes
	};
	tokio::task::spawn_blocking(move || {
		task.line(t!(
			"task.unpacking",
			count = indexes.len(),
			folder = output_dir.display()
		));
		task.progress(0.0);
		let summary = package.extract_many(
			&indexes,
			&output_dir,
			|done, total| {
				task.progress(done as f32 / total.max(1) as f32);
				if task.is_cancelled() {
					ControlFlow::Break(())
				} else {
					ControlFlow::Continue(())
				}
			},
			|error| task.line(t!("task.warning", error = error)),
		);
		if summary.cancelled {
			task.line(t!("task.unpack_cancelled", count = summary.extracted));
			task.finish(TaskState::Cancelled);
			return;
		}
		task.line(t!(
			"task.unpacked",
			count = summary.extracted,
			failed = summary.failed
		));
		task.line(output_dir.display().to_string());
		task.add_output(output_dir);
		task.finish(if summary.failed > 0 {
			TaskState::Failed
		} else {
			TaskState::Succeeded
		});
	});
}

pub(super) fn inspect_model(
	context: &Context,
	request: RequestId,
	target: Target,
	source: ModelSource,
) {
	let games = context.games.clone();
	let sink = context.sink.clone();
	tokio::task::spawn_blocking(move || {
		let mounts = vfs::mounts(&games, &target);
		let vfs = Vfs::build(mounts, |_| ControlFlow::Continue(()))
			.expect("building is only stopped by the callback");
		let result = inspect::inspect(&vfs, &source).map(Arc::new);
		sink.emit(Event::ModelInspected { request, result });
	});
}

pub(super) fn decompile(
	context: &Context,
	target: Option<Target>,
	source: ModelSource,
	output_dir: PathBuf,
) {
	let name = match &source {
		ModelSource::File(path) => path
			.file_name()
			.map(|n| n.to_string_lossy().into_owned())
			.unwrap_or_default(),
		ModelSource::Game(path) => path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned(),
	};
	let task = context.tasks.start(
		TaskKind::Decompile,
		t!("task.decompile", name = name).into_owned(),
	);
	let games = context.games.clone();
	tokio::task::spawn_blocking(move || {
		let vfs = match (&source, &target) {
			(ModelSource::Game(_), Some(target)) => {
				task.line(t!("task.search_where"));
				Vfs::build(vfs::mounts(&games, target), |_| ControlFlow::Continue(()))
			}
			_ => None,
		};
		match decompile::run(vfs.as_ref(), &source, &output_dir) {
			Ok(files) => {
				for file in &files {
					task.line(file.display().to_string());
				}
				task.line(t!("task.decompiled", count = files.len()));
				task.line(t!("task.decompile_note"));
				task.add_output(output_dir);
				task.finish(TaskState::Succeeded);
			}
			Err(e) => task.fail(e),
		}
	});
}

pub(super) fn search(context: &Context, request: RequestId, target: Target, needle: &str) {
	let needle = vfs::normalize(needle);
	let task = context.tasks.start(
		TaskKind::Search,
		t!("task.search", needle = needle, game = target.app.name).into_owned(),
	);
	let games = context.games.clone();
	let sink = context.sink.clone();
	tokio::task::spawn_blocking(move || {
		task.line(t!("task.search_where"));
		task.progress(0.0);
		let mounts = vfs::mounts(&games, &target);
		let built = Vfs::build(mounts, |done| {
			task.progress(done * 0.9);
			if task.is_cancelled() {
				ControlFlow::Break(())
			} else {
				ControlFlow::Continue(())
			}
		});
		let Some(vfs) = built else {
			task.finish(TaskState::Cancelled);
			return;
		};
		for (file, error) in vfs.failures() {
			task.line(t!(
				"task.warning",
				error = format!("{}: {error}", file.display())
			));
		}
		let matches = vfs.search(&needle, MAX_SEARCH_RESULTS);
		task.line(t!(
			"task.search_found",
			found = matches.len(),
			mounts = vfs.mounts.len(),
			files = vfs.file_count()
		));
		if matches.len() >= MAX_SEARCH_RESULTS {
			task.line(t!("task.search_limit", limit = MAX_SEARCH_RESULTS));
		}
		sink.emit(Event::SearchResults {
			request,
			task: task.id(),
			matches,
		});
		task.progress(1.0);
		task.finish(TaskState::Succeeded);
	});
}
