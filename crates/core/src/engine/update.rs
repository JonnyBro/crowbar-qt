use rust_i18n::t;

use super::{Context, Event, TaskKind};
use crate::app_update::{self, Release};
use crate::workshop::Client;

pub(super) async fn check(context: Context) {
	let result = match Client::new() {
		Ok(client) => app_update::check(&client).await,
		Err(e) => Err(e.to_string()),
	};
	match result {
		Ok(Some(release)) => {
			tracing::info!("Prybar {} is out", release.version);
			context.sink.emit(Event::UpdateAvailable(release));
		}
		Ok(None) => {}
		Err(e) => tracing::warn!("update check failed: {e}"),
	}
}

pub(super) async fn install(context: Context, release: Release) {
	let task = context.tasks.start(
		TaskKind::Update,
		t!("task.update", version = release.version).into_owned(),
	);
	let work_dir = match context.scratch_dir("update") {
		Ok(dir) => dir,
		Err(e) => return task.fail(e),
	};
	let client = match Client::new() {
		Ok(client) => client,
		Err(e) => return task.fail(e),
	};
	let progress = |received: u64, total: Option<u64>| {
		if let Some(total) = total.filter(|&t| t > 0) {
			task.progress(received as f32 / total as f32);
		}
	};
	let result =
		app_update::install(&client, &release, &work_dir, progress, task.cancelled()).await;
	let _ = std::fs::remove_dir_all(&work_dir);
	match result {
		Ok(path) => {
			task.line(t!("update.installed", path = path.display()));
			task.finish(super::TaskState::Succeeded);
			context.sink.emit(Event::UpdateInstalled);
		}
		Err(e) => task.fail(e),
	}
}
