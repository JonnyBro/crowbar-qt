use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use super::*;
use crate::games::Target;
use crate::steam::App;
use crate::studiomdl;

const TIMEOUT: Duration = Duration::from_secs(10);

fn write(path: &Path, content: &[u8]) {
	std::fs::create_dir_all(path.parent().unwrap()).unwrap();
	std::fs::write(path, content).unwrap();
}

fn script(path: &Path, body: &str) {
	write(path, format!("#!/bin/sh\n{body}\n").as_bytes());
	std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Harness {
	_temp: tempfile::TempDir,
	dir: PathBuf,
	paths: AppPaths,
	engine: Engine,
	/// Events already taken off the engine but not looked at yet.
	seen: Vec<Event>,
}

impl Harness {
	fn new() -> Self {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path().canonicalize().unwrap();
		let paths = AppPaths::under(dir.join("app"));
		paths.ensure_exist().unwrap();
		let engine = Engine::start(paths.clone(), Settings::default(), Arc::new(|| {})).unwrap();
		Self {
			_temp: temp,
			dir,
			paths,
			engine,
			seen: Vec::new(),
		}
	}

	/// A native game whose tools folder has the given shell scripts.
	fn target(&self, tools: &[(&str, &str)]) -> Target {
		let install = self.dir.join("game");
		for (name, body) in tools {
			script(&install.join("bin").join(name), body);
		}
		std::fs::create_dir_all(install.join("mod")).unwrap();
		Target {
			app: App {
				id: 4000,
				name: "Test Game".into(),
				install_path: install.clone(),
				tools_path: Some(install.join("bin")),
				game_dirs: vec![install.join("mod")],
				..App::default()
			},
			game_dir: install.join("mod"),
		}
	}

	/// Waits for the first event `pick` accepts, keeping the others for later.
	fn wait<T>(&mut self, mut pick: impl FnMut(&Event) -> Option<T>) -> T {
		let deadline = Instant::now() + TIMEOUT;
		loop {
			self.seen.extend(self.engine.poll_events());
			if let Some(index) = self.seen.iter().position(|e| pick(e).is_some()) {
				let event = self.seen.remove(index);
				return pick(&event).unwrap();
			}
			assert!(
				Instant::now() < deadline,
				"timed out; events: {:#?}",
				self.seen
			);
			std::thread::sleep(Duration::from_millis(10));
		}
	}

	fn finished(&mut self, kind: TaskKind) -> TaskSnapshot {
		self.wait(|e| match e {
			Event::TaskUpdated(s) if s.kind == kind && s.state.is_finished() => Some(s.clone()),
			_ => None,
		})
	}

	fn log(&mut self, id: TaskId) -> Vec<String> {
		self.seen.extend(self.engine.poll_events());
		self.seen
			.iter()
			.filter_map(|e| match e {
				Event::TaskLog { id: log_id, line } if *log_id == id => Some(line.clone()),
				_ => None,
			})
			.collect()
	}
}

#[test]
fn settings_are_saved_through_the_engine() {
	let mut h = Harness::new();
	let settings = Settings {
		tool_environment: "MANGOHUD=0".into(),
		..Settings::default()
	};
	h.engine.send(Command::SaveSettings(settings.clone()));
	let saved = h.wait(|e| match e {
		Event::SettingsSaved(s) => Some(s.clone()),
		_ => None,
	});
	assert_eq!(saved, settings);
	assert_eq!(Settings::load(&h.paths).unwrap(), settings);
}

#[test]
fn compile_moves_the_model_and_logs_output() {
	let mut h = Harness::new();
	// Called as: studiomdl -game <dir> -nop4 <qc>
	let target = h.target(&[(
		"studiomdl",
		"mkdir -p \"$2/models/props\" && echo mdl > \"$2/models/props/box.mdl\" && echo \"compiled $4\"",
	)]);
	let qc = h.dir.join("src/box.qc");
	write(&qc, b"$modelname \"props/box.mdl\"\n");
	let out = h.dir.join("out");

	h.engine.send(Command::Compile {
		target: target.clone(),
		files: vec![qc],
		options: studiomdl::Options::default(),
		output: CompileOutput::Folder(out.clone()),
		view_after: false,
	});
	let done = h.finished(TaskKind::Compile);
	assert_eq!(done.state, TaskState::Succeeded, "{:?}", h.log(done.id));
	assert_eq!(done.outputs, [out.join("models/props/box.mdl")]);
	assert_eq!(done.input, Some(h.dir.join("src/box.qc")));
	assert!(out.join("models/props/box.mdl").is_file());
	assert!(!target.game_dir.join("models/props/box.mdl").exists());
	assert!(h.log(done.id).contains(&"compiled box.qc".to_owned()));
}

#[test]
fn failed_compile_does_not_stop_the_queue() {
	let mut h = Harness::new();
	let target = h.target(&[("studiomdl", "test \"$4\" = good.qc")]);
	let bad = h.dir.join("bad.qc");
	let good = h.dir.join("good.qc");
	write(&bad, b"");
	write(&good, b"");

	h.engine.send(Command::Compile {
		target,
		files: vec![bad, good],
		options: studiomdl::Options::default(),
		output: CompileOutput::GameModels,
		view_after: false,
	});
	assert_eq!(h.finished(TaskKind::Compile).state, TaskState::Failed);
	assert_eq!(h.finished(TaskKind::Compile).state, TaskState::Succeeded);
}

#[test]
fn cancelling_stops_the_tool_and_the_queue() {
	let mut h = Harness::new();
	let target = h.target(&[("studiomdl", "sleep 30")]);
	let first = h.dir.join("a.qc");
	let second = h.dir.join("b.qc");
	write(&first, b"");
	write(&second, b"");

	let started = Instant::now();
	h.engine.send(Command::Compile {
		target,
		files: vec![first, second],
		options: studiomdl::Options::default(),
		output: CompileOutput::GameModels,
		view_after: false,
	});
	let running = h.wait(|e| match e {
		Event::TaskUpdated(s) if s.kind == TaskKind::Compile => Some(s.id),
		_ => None,
	});
	std::thread::sleep(Duration::from_millis(200));
	h.engine.send(Command::Cancel(running));
	let done = h.finished(TaskKind::Compile);
	assert_eq!(done.id, running);
	assert_eq!(done.state, TaskState::Cancelled);
	assert!(started.elapsed() < Duration::from_secs(5));

	// The second file never starts.
	std::thread::sleep(Duration::from_millis(300));
	h.seen.extend(h.engine.poll_events());
	assert!(
		!h.seen
			.iter()
			.any(|e| matches!(e, Event::TaskUpdated(s) if s.id != running))
	);
}

fn gma(entries: &[(&str, &[u8])]) -> Vec<u8> {
	let mut data = b"GMAD".to_vec();
	data.push(3);
	data.extend_from_slice(&[0; 16]);
	for text in ["", "Addon", "{}", "Author"] {
		data.extend_from_slice(text.as_bytes());
		data.push(0);
	}
	data.extend_from_slice(&1i32.to_le_bytes());
	for (i, (path, content)) in entries.iter().enumerate() {
		data.extend_from_slice(&(i as u32 + 1).to_le_bytes());
		data.extend_from_slice(path.as_bytes());
		data.push(0);
		data.extend_from_slice(&(content.len() as i64).to_le_bytes());
		data.extend_from_slice(&0u32.to_le_bytes());
	}
	data.extend_from_slice(&0u32.to_le_bytes());
	for (_, content) in entries {
		data.extend_from_slice(content);
	}
	data
}

#[test]
fn open_read_and_unpack_a_package() {
	let mut h = Harness::new();
	let file = h.dir.join("addon.gma");
	write(
		&file,
		&gma(&[("lua/a.lua", b"print(1)"), ("materials/b.vmt", b"vmt")]),
	);

	h.engine.send(Command::OpenPackage {
		request: 7,
		path: file,
	});
	let package = h.wait(|e| match e {
		Event::PackageOpened { request: 7, result } => Some(result.clone()),
		_ => None,
	});
	let package = package.unwrap();
	assert_eq!(package.entries.len(), 2);

	h.engine.send(Command::ReadEntry {
		request: 8,
		package: package.clone(),
		index: 0,
		max_size: 100,
	});
	let data = h.wait(|e| match e {
		Event::EntryRead {
			request: 8, result, ..
		} => Some(result.clone()),
		_ => None,
	});
	assert_eq!(data.unwrap().as_slice(), b"print(1)");

	let out = h.dir.join("unpacked");
	h.engine.send(Command::Unpack {
		package,
		indexes: Vec::new(),
		output_dir: out.clone(),
	});
	let done = h.finished(TaskKind::Unpack);
	assert_eq!(done.state, TaskState::Succeeded);
	assert_eq!(std::fs::read(out.join("materials/b.vmt")).unwrap(), b"vmt");
}

#[test]
fn search_finds_files_the_game_loads() {
	let mut h = Harness::new();
	let target = h.target(&[]);
	write(
		&target.game_dir.join("addons/one.gma"),
		&gma(&[("models/Crate.mdl", b"x"), ("lua/init.lua", b"y")]),
	);
	write(&target.game_dir.join("models/crate.mdl"), b"loose");
	write(
		&target.game_dir.join("addons/two.gma"),
		&gma(&[("sound/a.wav", b"z")]),
	);

	h.engine.send(Command::SearchPackages {
		request: 1,
		target: target.clone(),
		needle: "MODELS\\crate".into(),
	});
	let matches = h.wait(|e| match e {
		Event::SearchResults { matches, .. } => Some(matches.clone()),
		_ => None,
	});
	assert_eq!(matches.len(), 1);
	assert_eq!(matches[0].path, "models/Crate.mdl");
	let files: Vec<_> = matches[0].sources.iter().map(|s| s.file()).collect();
	assert_eq!(
		files,
		[
			target.game_dir.join("addons/one.gma"),
			target.game_dir.join("models/crate.mdl"),
		]
	);
	assert_eq!(h.finished(TaskKind::Search).state, TaskState::Succeeded);
}

#[test]
fn inspects_a_model_with_the_games_files() {
	let mut h = Harness::new();
	let target = h.target(&[]);
	let model = crate::mdl::tests::build(&crate::mdl::tests::Spec {
		materials: &["crate"],
		folders: &["models/"],
		skins: &[&[0]],
		body_parts: &[("body", &[("ref", 3, &[0])])],
		..Default::default()
	});
	write(&target.game_dir.join("models/crate.mdl"), &model);

	h.engine.send(Command::InspectModel {
		request: 7,
		target: target.clone(),
		source: crate::inspect::ModelSource::Game("models/crate.mdl".into()),
	});
	let (request, result) = h.wait(|e| match e {
		Event::ModelInspected { request, result } => Some((*request, result.clone())),
		_ => None,
	});
	assert_eq!(request, 7);
	let report = result.unwrap();
	assert_eq!(report.materials[0].tried, ["materials/models/crate.vmt"]);
	// No .vvd, no .vtx, no material.
	assert_eq!(report.problem_count(), 3);

	h.engine.send(Command::InspectModel {
		request: 8,
		target,
		source: crate::inspect::ModelSource::Game("models/none.mdl".into()),
	});
	let result = h.wait(|e| match e {
		Event::ModelInspected { request: 8, result } => Some(result.clone()),
		_ => None,
	});
	assert!(result.is_err());
}

#[test]
fn errors_fail_the_task_with_a_message() {
	let mut h = Harness::new();
	let target = h.target(&[]);
	h.engine.send(Command::View { target, mdl: None });
	let done = h.finished(TaskKind::View);
	assert_eq!(done.state, TaskState::Failed);
	let log = h.log(done.id);
	assert!(log.iter().any(|line| line.contains("hlmv")), "{log:?}");
}
