use std::path::Path;

use super::*;
use crate::steam::App;
use crate::{gma_writer, vpk_writer};

fn write(path: &Path, content: &[u8]) {
	std::fs::create_dir_all(path.parent().unwrap()).unwrap();
	std::fs::write(path, content).unwrap();
}

fn vpk(output: &Path, files: &[(&str, &[u8])]) {
	let staging = tempfile::tempdir().unwrap();
	for (path, content) in files {
		write(&staging.path().join(path), content);
	}
	vpk_writer::pack(
		staging.path(),
		output,
		vpk_writer::Options::default(),
		|_, _, _| {},
		|| false,
	)
	.unwrap();
}

fn gma(output: &Path, files: &[(&str, &[u8])]) {
	let staging = tempfile::tempdir().unwrap();
	write(
		&staging.path().join("addon.json"),
		br#"{"title": "Test", "type": "tool", "tags": ["fun"]}"#,
	);
	for (path, content) in files {
		write(&staging.path().join(path), content);
	}
	std::fs::create_dir_all(output.parent().unwrap()).unwrap();
	gma_writer::pack(staging.path(), output, false, |_, _, _| {}, || false).unwrap();
}

fn target(id: u32, install: &Path, game_dir: &Path) -> Target {
	Target {
		app: App {
			id,
			name: "Game".into(),
			install_path: install.to_path_buf(),
			game_dirs: vec![game_dir.to_path_buf()],
			..App::default()
		},
		game_dir: game_dir.to_path_buf(),
	}
}

fn paths(mounts: &[Mount], root: &Path) -> Vec<String> {
	mounts
		.iter()
		.map(|m| {
			m.path
				.strip_prefix(root)
				.unwrap()
				.to_string_lossy()
				.into_owned()
		})
		.collect()
}

fn build(mounts: Vec<Mount>) -> Vfs {
	Vfs::build(mounts, |_| ControlFlow::Continue(())).unwrap()
}

fn winners(found: &Found, root: &Path) -> Vec<String> {
	found
		.sources
		.iter()
		.map(|s| {
			s.file()
				.strip_prefix(root)
				.unwrap()
				.to_string_lossy()
				.into_owned()
		})
		.collect()
}

#[test]
fn source_game_follows_gameinfo() {
	let temp = tempfile::tempdir().unwrap();
	let root = temp.path();
	let game = root.join("mymod");
	write(
		&game.join("gameinfo.txt"),
		br#""GameInfo"
{
	game "Test"
	FileSystem
	{
		SearchPaths
		{
			game+mod			|gameinfo_path|custom/*
			game				|gameinfo_path|pak01.vpk
			mod+mod_write		|gameinfo_path|.
			game_lv				hl2\lv.vpk
			game				|all_source_engine_paths|HL2
			platform			platform
		}
	}
}"#,
	);
	write(&root.join("hl2/materials/x.vmt"), b"base");
	write(&root.join("hl2/lv_dir.vpk"), b"");
	write(&root.join("platform/p.txt"), b"");
	write(&game.join("custom/b_folder/materials/x.vmt"), b"custom");
	vpk(&game.join("custom/a.vpk"), &[("models/m.mdl", b"a")]);
	write(&game.join("custom/c_000.vpk"), b"data part");
	vpk(&game.join("custom/c_dir.vpk"), &[("models/c.mdl", b"c")]);
	vpk(
		&game.join("pak01_dir.vpk"),
		&[("models/m.mdl", b"pak"), ("sound/s.wav", b"s")],
	);

	let target = target(220, root, &game);
	let mounts = mounts(&Games::default(), &target);
	assert_eq!(
		paths(&mounts, root),
		[
			"mymod/custom/a.vpk",
			"mymod/custom/b_folder",
			"mymod/custom/c_dir.vpk",
			"mymod/pak01_dir.vpk",
			"hl2",
		]
	);

	let vfs = build(mounts);
	assert_eq!(vfs.failures().count(), 0);
	let found = vfs.find("MATERIALS\\X.VMT").unwrap();
	assert_eq!(found.path, "materials/x.vmt");
	assert_eq!(
		winners(&found, root),
		[
			"mymod/custom/b_folder/materials/x.vmt",
			"hl2/materials/x.vmt",
		]
	);
	let found = vfs.find("models/m.mdl").unwrap();
	assert_eq!(
		winners(&found, root),
		["mymod/custom/a.vpk", "mymod/pak01_dir.vpk"]
	);
	assert_eq!(found.sources[0].path, "models/m.mdl");
	assert_eq!(found.sources[0].size, 1);
	assert!(vfs.find("p.txt").is_none());

	let results: Vec<String> = vfs.search("mdl", 10).into_iter().map(|f| f.path).collect();
	assert_eq!(results, ["models/c.mdl", "models/m.mdl"]);
	assert_eq!(vfs.search("m", 1).len(), 1);
}

#[test]
fn no_gameinfo_means_the_folder_alone() {
	let temp = tempfile::tempdir().unwrap();
	let game = temp.path().join("game");
	write(&game.join("a.txt"), b"");
	let mounts = mounts(&Games::default(), &target(220, temp.path(), &game));
	assert_eq!(paths(&mounts, temp.path()), ["game"]);
}

#[test]
fn garrys_mod_order() {
	let temp = tempfile::tempdir().unwrap();
	let root = temp.path();
	let install = root.join("common/GarrysMod");
	let game = install.join("garrysmod");
	let library = root.join("library");
	let workshop = library.join("steamapps/workshop/content/4000");

	write(&game.join("addons/legacy/materials/x.vmt"), b"legacy");
	gma(
		&game.join("addons/legacy/inner.gma"),
		&[("materials/x.vmt", b"inner")],
	);
	gma(
		&game.join("addons/old_123.gma"),
		&[("materials/x.vmt", b"off")],
	);
	gma(
		&game.join("addons/kept_456.gma"),
		&[("materials/x.vmt", b"kept")],
	);
	gma(
		&workshop.join("1000/item.gma"),
		&[("materials/x.vmt", b"1000")],
	);
	gma(
		&workshop.join("999/item.gma"),
		&[("materials/x.vmt", b"999")],
	);
	gma(
		&workshop.join("777/item.gma"),
		&[("materials/x.vmt", b"off")],
	);
	write(
		&game.join("cfg/addonnomount.txt"),
		b"\"addonnomount\"\n{\n\t\"1\" \"123\"\n\t\"2\" \"777\"\n}\n",
	);
	write(
		&game.join("cfg/mountdepots.txt"),
		b"\"gamedepotsystem\"\n{\n\t\"cstrike\" \"1\"\n\t\"tf\" \"0\"\n}\n",
	);
	vpk(
		&game.join("garrysmod_dir.vpk"),
		&[("materials/x.vmt", b"vpk")],
	);
	write(&game.join("materials/x.vmt"), b"loose");
	vpk(
		&install.join("sourceengine/content_a_dir.vpk"),
		&[("materials/x.vmt", b"content")],
	);
	vpk(
		&install.join("sourceengine/hl2_misc_dir.vpk"),
		&[("materials/x.vmt", b"misc")],
	);
	write(&install.join("platform/platform_misc_dir.vpk"), b"");
	write(&game.join("download/materials/x.vmt"), b"download");
	write(&game.join("fallbacks/materials/x.vmt"), b"fallback");

	let css = root.join("common/css/cstrike");
	write(
		&css.join("gameinfo.txt"),
		b"\"GameInfo\" { FileSystem { SearchPaths { game |gameinfo_path|. } } }",
	);
	let tf = root.join("common/tf2/tf");
	write(&tf.join("gameinfo.txt"), b"");

	let games = Games {
		apps: vec![
			target(240, &root.join("common/css"), &css).app,
			target(440, &root.join("common/tf2"), &tf).app,
		],
		libraries: vec![library],
		..Games::default()
	};
	let mounts = mounts(&games, &target(GMOD_APP_ID, &install, &game));
	assert_eq!(
		paths(&mounts, root),
		[
			"common/GarrysMod/garrysmod/addons/legacy",
			"common/GarrysMod/garrysmod/addons/legacy/inner.gma",
			"common/GarrysMod/garrysmod/addons/kept_456.gma",
			"library/steamapps/workshop/content/4000/999/item.gma",
			"library/steamapps/workshop/content/4000/1000/item.gma",
			"common/GarrysMod/garrysmod/garrysmod_dir.vpk",
			"common/GarrysMod/garrysmod",
			"common/GarrysMod/sourceengine/hl2_misc_dir.vpk",
			"common/GarrysMod/sourceengine/content_a_dir.vpk",
			"common/GarrysMod/sourceengine",
			"common/GarrysMod/platform/platform_misc_dir.vpk",
			"common/GarrysMod/platform",
			"common/css/cstrike",
			"common/GarrysMod/garrysmod/download",
			"common/GarrysMod/garrysmod/fallbacks",
		]
	);
}

#[test]
fn bad_packages_are_reported_and_skipped() {
	let temp = tempfile::tempdir().unwrap();
	let bad = temp.path().join("bad_dir.vpk");
	write(&bad, b"not a vpk");
	let folder = temp.path().join("loose");
	write(&folder.join("a.txt"), b"abc");
	let vfs = build(vec![
		Mount {
			path: bad.clone(),
			kind: MountKind::Package,
		},
		Mount {
			path: folder,
			kind: MountKind::Folder,
		},
	]);
	assert_eq!(
		vfs.failures().map(|(p, _)| p).collect::<Vec<_>>(),
		[bad.as_path()]
	);
	assert_eq!(vfs.find("a.txt").unwrap().sources[0].size, 3);
	assert_eq!(vfs.file_count(), 1);
}

#[test]
fn building_can_be_cancelled() {
	let mounts = vec![Mount {
		path: PathBuf::from("/nonexistent"),
		kind: MountKind::Folder,
	}];
	assert!(Vfs::build(mounts, |_| ControlFlow::Break(())).is_none());
}

#[test]
fn normalizes_paths() {
	assert_eq!(
		normalize(" Materials\\\\Models/./A.VMT "),
		"materials/models/a.vmt"
	);
	assert_eq!(gma_id("title_12345.gma"), "12345");
	assert_eq!(
		strip_prefix_ci("|GameInfo_Path|x", "|gameinfo_path|"),
		Some("x")
	);
}
