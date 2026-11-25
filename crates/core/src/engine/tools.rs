//! Jobs that run one of the game's tools: compiling, viewing, packing, starting
//! the game or Hammer.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::SystemTime;

use rust_i18n::t;

use super::Context;
use super::messages::CompileOutput;
use super::task::{Task, TaskKind, TaskState, new_cancel};
use crate::games::Target;
use crate::launcher::Command;
use crate::process::{self, Exit};
use crate::{gma_writer, packer, runner, studiomdl, vpk_writer};

fn file_name(path: &Path) -> String {
	path.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_else(|| path.display().to_string())
}

/// Logs the command line, runs it and logs its output. `None` if it could not start.
pub(super) async fn run_tool(task: &Task, command: &Command) -> Option<Exit> {
	run_tool_with(task, command, |_| {}).await
}

/// Like [`run_tool`]; `on_line` also sees every output line.
pub(super) async fn run_tool_with(
	task: &Task,
	command: &Command,
	mut on_line: impl FnMut(&str),
) -> Option<Exit> {
	task.line(command.display_string());
	task.line("");
	let output = |line: String| {
		on_line(&line);
		task.line(line);
	};
	match process::run(command, output, task.cancelled()).await {
		Ok(exit) => Some(exit),
		Err(e) => {
			task.fail(t!(
				"task.start_failed",
				program = command.program.display(),
				error = e
			));
			None
		}
	}
}

/// The state a console tool's exit leads to, logging why it failed.
pub(super) fn exit_state(task: &Task, exit: Exit) -> TaskState {
	if exit.cancelled {
		task.line(t!("task.cancelled"));
		TaskState::Cancelled
	} else if exit.success {
		TaskState::Succeeded
	} else {
		task.line(t!(
			"task.exit_code",
			code = exit.code.map_or_else(|| "?".to_owned(), |c| c.to_string())
		));
		TaskState::Failed
	}
}

pub(super) async fn compile(
	context: Context,
	target: Target,
	files: Vec<PathBuf>,
	options: studiomdl::Options,
	output: CompileOutput,
	view_after: bool,
) {
	let cancel = new_cancel();
	let mut succeeded = 0;
	let mut last_model = None;
	for qc in files {
		if *cancel.borrow() {
			break;
		}
		let task = context.tasks.start_with(
			TaskKind::Compile,
			t!("task.compile", name = file_name(&qc)).into_owned(),
			cancel.clone(),
			Some(qc.clone()),
		);
		let output_dir = output.dir_for(&qc);
		let command = match studiomdl::compile_command(
			&target.app,
			&target.game_dir,
			&qc,
			&options,
			context.user_env(),
		) {
			Ok(command) => command,
			Err(e) => {
				task.fail(e);
				continue;
			}
		};
		let model_name = studiomdl::read_model_name(&qc);
		let Some(exit) = run_tool(&task, &command).await else {
			continue;
		};
		let state = exit_state(&task, exit);
		if state != TaskState::Succeeded {
			task.finish(state);
			continue;
		}

		if let Some(output_dir) = &output_dir {
			task.line("");
			match &model_name {
				None => task.line(t!("studio.no_modelname")),
				Some(name) => {
					let moved = studiomdl::move_compiled_model(&target.game_dir, name, output_dir);
					for line in moved.log {
						task.line(line);
					}
				}
			}
		}
		let model = model_name.as_deref().and_then(|name| {
			studiomdl::compiled_model(&target.game_dir, output_dir.as_deref(), name)
		});
		if let Some(model) = &model {
			task.add_output(model.clone());
		}
		succeeded += 1;
		last_model = model;
		task.finish(TaskState::Succeeded);
	}

	// Several viewer windows at once would only get in the way.
	if view_after && succeeded == 1 && !*cancel.borrow() {
		match last_model {
			Some(model) => view(context, target, Some(model)).await,
			None => {
				let task = context
					.tasks
					.start(TaskKind::View, t!("task.hlmv_plain").into_owned());
				task.fail(t!("task.no_compiled_model"));
			}
		}
	}
}

/// HLMV; closing it is the normal way for the task to end.
pub(super) async fn view(context: Context, target: Target, mdl: Option<PathBuf>) {
	let title = match &mdl {
		Some(mdl) => t!("task.hlmv", name = file_name(mdl)),
		None => t!("task.hlmv_plain"),
	};
	let task = context.tasks.start(TaskKind::View, title.into_owned());
	let command = match runner::hlmv_command(
		&target.app,
		&target.game_dir,
		mdl.as_deref(),
		context.user_env(),
	) {
		Ok(command) => command,
		Err(e) => return task.fail(e),
	};
	if let Some(exit) = run_tool(&task, &command).await {
		task.finish(if exit.cancelled {
			TaskState::Cancelled
		} else {
			TaskState::Succeeded
		});
	}
}

pub(super) async fn hammer(context: Context, target: Target) {
	let task = context.tasks.start(
		TaskKind::Hammer,
		t!("task.run", name = "Hammer").into_owned(),
	);
	let command = match runner::hammer_command(&target.app, &target.game_dir, context.user_env()) {
		Ok(command) => command,
		Err(e) => return task.fail(e),
	};
	if let Some(exit) = run_tool(&task, &command).await {
		task.finish(if exit.cancelled {
			TaskState::Cancelled
		} else {
			TaskState::Succeeded
		});
	}
}

/// Steam starts the game and returns at once, or keeps running when Steam itself
/// wasn't; either way the task ends once Steam has the request.
pub(super) async fn run_game(context: Context, target: Target, options: Vec<String>) {
	let name = target.app.name.clone();
	let task = context
		.tasks
		.start(TaskKind::RunGame, t!("task.run", name = name).into_owned());
	let command = match runner::game_command(&target.app, Some(&target.game_dir), &options) {
		Ok(command) => command,
		Err(e) => return task.fail(e),
	};
	task.line(command.display_string());
	let spawned = std::process::Command::new(&command.program)
		.args(&command.args)
		.env_clear()
		.envs(&command.env)
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.process_group(0)
		.spawn();
	match spawned {
		Ok(mut child) => {
			// Reaped in the background so it doesn't linger as a zombie.
			tokio::task::spawn_blocking(move || child.wait());
			task.line(t!("task.asked_steam", name = name));
			task.finish(TaskState::Succeeded);
		}
		Err(e) => task.fail(t!(
			"task.start_failed",
			program = command.program.display(),
			error = e
		)),
	}
}

pub(super) async fn pack_job(
	context: Context,
	target: Option<Target>,
	format: packer::Format,
	folder: PathBuf,
	output: PathBuf,
	options: packer::Options,
) {
	let title = format!("{}.{}", file_name(&folder), format.extension());
	let task = context
		.tasks
		.start(TaskKind::Pack, t!("task.pack", name = title).into_owned());
	pack(
		&context,
		&task,
		target.as_ref(),
		format,
		&folder,
		&output,
		&options,
	)
	.await;
}

/// Packs and finishes `task`; the package files when it worked.
pub(super) async fn pack(
	context: &Context,
	task: &Task,
	target: Option<&Target>,
	format: packer::Format,
	folder: &Path,
	output: &Path,
	options: &packer::Options,
) -> Option<Vec<PathBuf>> {
	if !context.settings.use_game_tools {
		return match format {
			packer::Format::Gma => pack_gma(task, folder, output, options.warn_invalid),
			packer::Format::Vpk => pack_vpk(task, folder, output, options),
		};
	}
	let Some(target) = target else {
		task.fail(t!("task.no_game"));
		return None;
	};
	let started = SystemTime::now();
	let command = match packer::pack_command(
		&target.app,
		format,
		folder,
		output,
		options,
		context.user_env(),
	) {
		Ok(command) => command,
		Err(e) => {
			task.fail(e);
			return None;
		}
	};
	let exit = run_tool(task, &command).await?;
	let state = exit_state(task, exit);
	if state != TaskState::Succeeded {
		task.finish(state);
		return None;
	}
	let result = packer::finish_pack(format, folder, output, options, started);
	for line in result.log {
		task.line(line);
	}
	if result.files.is_empty() {
		task.finish(TaskState::Failed);
		return None;
	}
	for file in &result.files {
		task.add_output(file.clone());
	}
	task.finish(TaskState::Succeeded);
	Some(result.files)
}

/// Packs with our own writer, so neither vpk nor a game is needed.
fn pack_vpk(
	task: &Task,
	folder: &Path,
	output: &Path,
	options: &packer::Options,
) -> Option<Vec<PathBuf>> {
	let vpk_options = match vpk_writer::Options::from_args(options.multi_chunk, &options.extra) {
		Ok(vpk_options) => vpk_options,
		Err(e) => {
			task.fail(e);
			return None;
		}
	};
	task.line(t!(
		"vpk.packing",
		folder = folder.display(),
		output = output.display()
	));
	// Same as for GMA: the writer is synchronous and keeps the task borrowed.
	let result = tokio::task::block_in_place(|| {
		vpk_writer::pack(
			folder,
			output,
			vpk_options,
			|name, done, total| {
				task.line(format!("\t{name}"));
				task.progress(done as f32 / total.max(1) as f32);
			},
			|| task.is_cancelled(),
		)
	});
	match result {
		Ok(packed) => {
			for path in &packed.skipped {
				task.line(t!("vpk.skipped", path = path));
			}
			for path in &packed.outputs {
				task.line(t!("vpk.saved", path = path.display()));
				task.add_output(path.clone());
			}
			task.line(t!("vpk.summary", n = packed.files, size = packed.data_size));
			task.finish(TaskState::Succeeded);
			Some(packed.outputs)
		}
		Err(vpk_writer::Error::Cancelled) => {
			task.finish(TaskState::Cancelled);
			None
		}
		Err(e) => {
			task.fail(e);
			None
		}
	}
}

/// Packs with our own writer, so neither gmad nor a game is needed.
fn pack_gma(task: &Task, folder: &Path, output: &Path, skip_invalid: bool) -> Option<Vec<PathBuf>> {
	task.line(t!(
		"gma.packing",
		folder = folder.display(),
		output = output.display()
	));
	// The writer reads files synchronously; this keeps the task borrowed
	// instead of moving it to a blocking thread.
	let result = tokio::task::block_in_place(|| {
		gma_writer::pack(
			folder,
			output,
			skip_invalid,
			|name, done, total| {
				task.line(format!("\t{name}"));
				task.progress(done as f32 / total.max(1) as f32);
			},
			|| task.is_cancelled(),
		)
	});
	match result {
		Ok(packed) => {
			for file in &packed.skipped {
				task.line(t!("gma.skipped", path = file));
			}
			for file in &packed.duplicates {
				task.line(t!("gma.duplicate", path = file));
			}
			task.line(t!(
				"gma.saved",
				path = output.display(),
				n = packed.files.len(),
				size = packed.size
			));
			task.add_output(output.to_path_buf());
			task.finish(TaskState::Succeeded);
			Some(vec![output.to_path_buf()])
		}
		Err(gma_writer::Error::Cancelled) => {
			task.finish(TaskState::Cancelled);
			None
		}
		Err(gma_writer::Error::InvalidFiles(files)) => {
			for file in &files {
				task.line(t!("gma.not_allowed", path = file));
			}
			task.fail(gma_writer::Error::InvalidFiles(files));
			None
		}
		Err(e) => {
			task.fail(e);
			None
		}
	}
}
