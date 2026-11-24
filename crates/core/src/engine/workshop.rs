//! Workshop jobs: your items, publishing, finding and downloading items.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rust_i18n::t;

use super::Context;
use super::messages::{Event, FoundItem, RequestId};
use super::task::{Task, TaskKind, TaskState};
use super::tools::{exit_state, pack, run_tool_with};
use crate::games::Target;
use crate::packer;
use crate::publisher::{self, ListParser};
use crate::workshop::{self, Client, ItemDetails};

/// Items per GetPublishedFileDetails request.
const DETAILS_BATCH: usize = 100;

fn client(task: Option<&Task>) -> Option<Client> {
	match Client::new() {
		Ok(client) => Some(client),
		Err(e) => {
			if let Some(task) = task {
				task.fail(e);
			}
			None
		}
	}
}

async fn fetch_details(client: &Client, ids: &[u64]) -> Result<Vec<ItemDetails>, workshop::Error> {
	let mut details = Vec::with_capacity(ids.len());
	for batch in ids.chunks(DETAILS_BATCH) {
		details.extend(client.fetch_details(batch).await?);
	}
	Ok(details)
}

pub(super) async fn list(context: Context, target: Target) {
	let task = context.tasks.start(
		TaskKind::WorkshopList,
		t!("task.workshop_list").into_owned(),
	);
	let command = match publisher::list_command(&target.app, context.user_env()) {
		Ok(command) => command,
		Err(e) => return task.fail(e),
	};
	let mut parser = ListParser::default();
	let Some(exit) = run_tool_with(&task, &command, |line| parser.add_line(line)).await else {
		return;
	};
	if exit.cancelled {
		return task.finish(exit_state(&task, exit));
	}
	if !exit.success {
		task.line(t!(
			"task.list_failed",
			code = exit.code.map_or_else(|| "?".to_owned(), |c| c.to_string())
		));
		return task.finish(TaskState::Failed);
	}

	let items = parser.items().to_vec();
	task.line(t!("task.items", count = items.len()));
	let ids: Vec<u64> = items.iter().map(|item| item.id).collect();
	context.sink.emit(Event::WorkshopItems(items));
	task.finish(TaskState::Succeeded);

	if ids.is_empty() {
		return;
	}
	let Some(client) = client(None) else {
		return;
	};
	match fetch_details(&client, &ids).await {
		Ok(details) => context.sink.emit(Event::WorkshopDetails(details)),
		Err(e) => tracing::warn!("workshop details: {e}"),
	}
}

pub(super) async fn publish(
	context: Context,
	target: Target,
	id: Option<u64>,
	content: PathBuf,
	icon: Option<PathBuf>,
	changes: String,
) {
	let title = match id {
		Some(id) => t!("task.update_item", id = id),
		None => t!("task.publish_new"),
	};
	let scratch = match context.scratch_dir("publish") {
		Ok(dir) => dir,
		Err(e) => {
			let task = context.tasks.start(TaskKind::Publish, title.into_owned());
			return task.fail(e);
		}
	};

	let mut converted = false;
	let icon = match icon {
		Some(icon) => {
			let out = scratch.join("icon.jpg");
			let source = icon.clone();
			let prepared =
				tokio::task::spawn_blocking(move || publisher::prepare_icon(&source, &out)).await;
			match prepared {
				Ok(Ok(prepared)) => {
					converted = prepared != icon;
					Some(prepared)
				}
				Ok(Err(e)) => {
					let task = context.tasks.start(TaskKind::Publish, title.into_owned());
					return task.fail(e);
				}
				Err(e) => {
					let task = context.tasks.start(TaskKind::Publish, title.into_owned());
					return task.fail(e);
				}
			}
		}
		None => None,
	};

	// A folder is packed with gmad first, into a temporary GMA.
	let gma = if content.is_dir() {
		let name = format!(
			"{}.gma",
			content
				.file_name()
				.map(|n| n.to_string_lossy().into_owned())
				.unwrap_or_else(|| "addon".into())
		);
		let packed = scratch.join(&name);
		let _ = std::fs::remove_file(&packed);
		let pack_task = context
			.tasks
			.start(TaskKind::Pack, t!("task.pack", name = name).into_owned());
		let options = packer::Options {
			warn_invalid: true,
			..packer::Options::default()
		};
		let files = pack(
			&context,
			&pack_task,
			&target,
			packer::Format::Gma,
			&content,
			&packed,
			&options,
		)
		.await;
		match files.and_then(|files| files.into_iter().next()) {
			Some(file) => Some(file),
			None => return,
		}
	} else if content.as_os_str().is_empty() {
		None
	} else {
		Some(content)
	};

	let task = context.tasks.start(TaskKind::Publish, title.into_owned());
	let request = publisher::Request {
		id,
		gma,
		icon,
		changes,
	};
	let command = match publisher::publish_command(&target.app, &request, context.user_env()) {
		Ok(command) => command,
		Err(e) => return task.fail(e),
	};
	if converted {
		task.line(t!("task.icon_converted"));
	}
	let mut created = None;
	let Some(exit) = run_tool_with(&task, &command, |line| {
		if let Some(id) = publisher::parse_created_id(line) {
			created = Some(id);
		}
	})
	.await
	else {
		return;
	};

	task.line("");
	if let Some(id) = created {
		task.line(t!(
			"task.created_item",
			id = id,
			url = workshop::item_url(id)
		));
		context.sink.emit(Event::Published {
			task: task.id(),
			id,
		});
	}
	if exit.cancelled {
		return task.finish(exit_state(&task, exit));
	}
	if exit.success {
		if created.is_none() {
			task.line(t!("task.publish_done"));
		}
		task.finish(TaskState::Succeeded);
	} else {
		task.line(t!(
			"task.publish_failed",
			code = exit.code.map_or_else(|| "?".to_owned(), |c| c.to_string())
		));
		task.finish(TaskState::Failed);
	}
}

pub(super) async fn look_up(context: Context, request: RequestId, text: String) {
	let result = find_items(&text).await;
	context.sink.emit(Event::ItemsFound { request, result });
}

async fn find_items(text: &str) -> Result<Vec<FoundItem>, String> {
	let ids = workshop::parse_item_ids(text);
	if ids.is_empty() {
		return Err(t!("task.no_ids").into_owned());
	}
	let client = Client::new().map_err(|e| e.to_string())?;

	let mut wanted: Vec<(u64, Option<u64>)> = Vec::new();
	for id in ids {
		match client.fetch_collection(id).await {
			Ok((children, true)) => {
				wanted.extend(children.into_iter().map(|child| (child, Some(id))));
			}
			// A failed lookup counts the ID as an ordinary item.
			_ => wanted.push((id, None)),
		}
	}
	let mut seen = std::collections::HashSet::new();
	wanted.retain(|(id, _)| seen.insert(*id));

	let ids: Vec<u64> = wanted.iter().map(|(id, _)| *id).collect();
	let details = fetch_details(&client, &ids)
		.await
		.map_err(|e| e.to_string())?;
	Ok(details
		.into_iter()
		.zip(wanted)
		.map(|(details, (_, collection))| FoundItem {
			details,
			collection,
		})
		.collect())
}

pub(super) async fn download(
	context: Context,
	items: Vec<ItemDetails>,
	output_dir: PathBuf,
	with_title: bool,
	convert: bool,
) {
	for item in items {
		let _turn = context.download_lock.lock().await;
		let name = if item.title.is_empty() {
			item.id.to_string()
		} else {
			item.title.clone()
		};
		let task = context.tasks.start(
			TaskKind::Download,
			t!("task.download", name = name).into_owned(),
		);
		if let Some(error) = &item.error {
			task.fail(error);
			continue;
		}
		let Some(files) = fetch_item(&context, &task, &item).await else {
			continue;
		};
		save(
			&context,
			&task,
			&item,
			&files,
			&output_dir,
			with_title,
			convert,
		);
	}
}

/// The item's files: Steam's cached copy, the direct download or steamcmd's.
async fn fetch_item(context: &Context, task: &Task, item: &ItemDetails) -> Option<Vec<PathBuf>> {
	// Steam's own copy is the newest the client has; it needs no network at all.
	let cached = workshop::find_cached_files(&context.games.libraries, item.app_id, item.id);
	if let Some(first) = cached.first() {
		let folder = first.parent().unwrap_or(first);
		task.line(t!("task.from_cache", folder = folder.display()));
		return Some(cached);
	}

	if !item.file_url.is_empty() {
		let client = client(Some(task))?;
		let dir = match context.scratch_dir("downloads") {
			Ok(dir) => dir,
			Err(e) => {
				task.fail(e);
				return None;
			}
		};
		let file = dir.join(format!("{}.download", item.id));
		task.line(t!("task.downloading", url = item.file_url));
		task.progress(0.0);
		let result = client
			.download(
				&item.file_url,
				&file,
				|received, total| {
					if let Some(total) = total.filter(|&t| t > 0) {
						task.progress(received as f32 / total as f32);
					}
				},
				task.cancelled(),
			)
			.await;
		return match result {
			Ok(()) => Some(vec![file]),
			Err(workshop::Error::Cancelled) => {
				task.line(t!("task.cancelled"));
				task.finish(TaskState::Cancelled);
				None
			}
			Err(e) => {
				task.fail(t!("task.download_failed", error = e));
				None
			}
		};
	}

	let Some(steamcmd) = workshop::find_steamcmd() else {
		task.fail(t!("task.no_direct_link"));
		return None;
	};
	let install_dir = match context.scratch_dir("steamcmd") {
		Ok(dir) => dir,
		Err(e) => {
			task.fail(e);
			return None;
		}
	};
	// Kept between runs, so steamcmd doesn't download and update itself every time.
	let home = context.paths.data_dir().join("steamcmd-home");
	let command = match workshop::steamcmd_command(
		Some(&steamcmd),
		&home,
		&install_dir,
		item.app_id,
		item.id,
	) {
		Ok(command) => command,
		Err(e) => {
			task.fail(e);
			return None;
		}
	};
	let mut downloaded = None;
	let exit = run_tool_with(task, &command, |line| {
		if let Some(path) = workshop::parse_steamcmd_path(line) {
			downloaded = Some(PathBuf::from(path));
		}
	})
	.await?;
	if exit.cancelled {
		task.finish(exit_state(task, exit));
		return None;
	}

	// steamcmd names the file for single-file items and the folder otherwise.
	let files = match downloaded {
		Some(path) if path.is_file() => vec![path],
		Some(path) => {
			let mut files: Vec<PathBuf> = std::fs::read_dir(&path)
				.map(|read| {
					read.flatten()
						.map(|e| e.path())
						.filter(|p| p.is_file())
						.collect()
				})
				.unwrap_or_default();
			files.sort();
			files
		}
		None => Vec::new(),
	};
	if files.is_empty() {
		task.line(t!("task.steamcmd_failed"));
		task.finish(TaskState::Failed);
		return None;
	}
	Some(files)
}

fn save(
	context: &Context,
	task: &Task,
	item: &ItemDetails,
	files: &[PathBuf],
	output_dir: &Path,
	with_title: bool,
	convert: bool,
) {
	let base_name = workshop::file_base_name(item, with_title);
	let mut ok = true;
	for (i, file) in files.iter().enumerate() {
		let name = if files.len() == 1 {
			base_name.clone()
		} else {
			format!("{base_name}_{}", i + 1)
		};
		match workshop::save_file(file, output_dir, &name, convert) {
			Ok(saved) => {
				task.line(t!("task.saved", path = saved.display()));
				task.add_output(saved);
				// Downloads can be gigabytes; drop our own copy now rather than at exit.
				if file.starts_with(&context.scratch.0) {
					let _ = std::fs::remove_file(file);
				}
			}
			Err(e) => {
				task.line(t!("task.error", error = e));
				ok = false;
			}
		}
	}
	task.finish(if ok {
		TaskState::Succeeded
	} else {
		TaskState::Failed
	});
}

pub(super) async fn fetch_image(context: Context, request: RequestId, url: String) {
	let result = match Client::new() {
		Ok(client) => client
			.fetch_bytes(&url)
			.await
			.map(Arc::new)
			.map_err(|e| e.to_string()),
		Err(e) => Err(e.to_string()),
	};
	context.sink.emit(Event::Image { request, result });
}
