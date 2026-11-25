//! Reading, unpacking and searching packages, off the async threads.

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;

use rust_i18n::t;

use super::Context;
use super::messages::{Event, RequestId, TEXTURE_PREVIEW_SIDE};
use super::task::{TaskKind, TaskState};
use crate::games::Target;
use crate::package::Package;
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

pub(super) fn search(context: &Context, request: RequestId, target: Target, needle: &str) {
	let needle = needle.trim().to_lowercase().replace('\\', "/");
	let task = context.tasks.start(
		TaskKind::Search,
		t!("task.search", needle = needle, game = target.app.name).into_owned(),
	);
	let games = context.games.clone();
	let sink = context.sink.clone();
	tokio::task::spawn_blocking(move || {
		task.line(t!("task.search_where"));
		task.progress(0.0);
		let packages = games.packages(&target);
		let mut found = 0;
		for (i, file) in packages.iter().enumerate() {
			if task.is_cancelled() || found >= MAX_SEARCH_RESULTS {
				break;
			}
			if let Ok(package) = Package::open(file) {
				let matches: Vec<(String, u64)> = package
					.entries
					.iter()
					.filter(|entry| entry.path.to_lowercase().contains(&needle))
					.take(MAX_SEARCH_RESULTS - found)
					.map(|entry| (entry.path.clone(), entry.size))
					.collect();
				if !matches.is_empty() {
					found += matches.len();
					sink.emit(Event::SearchResults {
						request,
						task: task.id(),
						package: file.clone(),
						matches,
					});
				}
			}
			task.progress((i + 1) as f32 / packages.len() as f32);
		}
		task.line(t!(
			"task.search_found",
			found = found,
			total = packages.len()
		));
		if found >= MAX_SEARCH_RESULTS {
			task.line(t!("task.search_limit", limit = MAX_SEARCH_RESULTS));
		}
		task.finish(if task.is_cancelled() {
			TaskState::Cancelled
		} else {
			TaskState::Succeeded
		});
	});
}
