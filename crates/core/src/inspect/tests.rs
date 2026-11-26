use std::ops::ControlFlow;

use super::*;
use crate::mdl::tests::{Spec, build};
use crate::vfs::Mount;

fn write(path: &Path, content: &[u8]) {
	std::fs::create_dir_all(path.parent().unwrap()).unwrap();
	std::fs::write(path, content).unwrap();
}

fn vvd(checksum: i32) -> Vec<u8> {
	let mut data = b"IDSV".to_vec();
	data.extend_from_slice(&4i32.to_le_bytes());
	data.extend_from_slice(&checksum.to_le_bytes());
	data
}

fn vtx(checksum: i32) -> Vec<u8> {
	let mut data = vec![0u8; 16];
	data.extend_from_slice(&checksum.to_le_bytes());
	data
}

fn game(folders: &[&Path]) -> Vfs {
	let mounts = folders
		.iter()
		.map(|f| Mount {
			path: f.to_path_buf(),
			kind: MountKind::Folder,
		})
		.collect();
	Vfs::build(mounts, |_| ControlFlow::Continue(())).unwrap()
}

fn crate_model() -> Vec<u8> {
	build(&Spec {
		checksum: 42,
		materials: &["Crate", "lid", "world", "patched", "gone", "leftover"],
		folders: &["models\\props\\", "models/shared/"],
		skins: &[&[0, 1, 2, 3, 4]],
		body_parts: &[("body", &[("ref", 10, &[0, 1, 2, 3, 4])])],
		includes: &["models/anims.mdl", "models/missing_anims.mdl"],
		..Spec::default()
	})
}

#[test]
fn checks_a_model_the_way_the_game_loads_it() {
	let temp = tempfile::tempdir().unwrap();
	let mine = temp.path().join("mine");
	let base = temp.path().join("base");
	write(&mine.join("models/props/crate.mdl"), &crate_model());
	write(&mine.join("models/props/crate.vvd"), &vvd(42));
	write(&mine.join("models/props/crate.dx90.vtx"), &vtx(42));
	write(&mine.join("models/props/crate.phy"), &[0; 16]);
	write(&base.join("models/anims.mdl"), b"IDST");

	write(
		&mine.join("materials/models/props/crate.vmt"),
		br#""VertexLitGeneric"
{
	"$basetexture" "models/props/crate"
	"$bumpmap" "models\props\crate_normal.vtf"
	"$envmap" "env_cubemap"
	"$envmapmask" "models/props/missing_mask"
	"$phongexponent" "20"
	"$color2" "[1 1 1]"
	"$surfaceprop" "wood"
}
"#,
	);
	write(&mine.join("materials/models/props/crate.vtf"), b"");
	write(&base.join("materials/models/props/crate_normal.vtf"), b"");
	write(
		&base.join("materials/models/shared/lid.vmt"),
		b"VertexLitGeneric { $basetexture models/props/crate }\n/* trailing junk {",
	);
	write(
		&mine.join("materials/models/props/world.vmt"),
		br#""LightmappedGeneric" { "$basetexture" "models/props/crate" }"#,
	);
	write(
		&mine.join("materials/models/props/patched.vmt"),
		br#"Patch { include "materials/models/props/crate.vmt" replace { "$basetexture" "models/props/other" } }"#,
	);

	let vfs = game(&[&mine, &base]);
	let report = inspect(&vfs, &ModelSource::Game("models\\props\\CRATE.MDL".into())).unwrap();
	assert_eq!(report.game_path.as_deref(), Some("models/props/crate.mdl"));
	assert!(report.game_copy.is_none());

	let state = |ext: &str| {
		let c = report
			.companions
			.iter()
			.find(|c| c.extension == ext)
			.unwrap();
		match &c.state {
			CompanionState::Found { matches, .. } => Some(*matches),
			_ => None,
		}
	};
	assert_eq!(state("vvd"), Some(true));
	assert_eq!(state("dx90.vtx"), Some(true));
	assert_eq!(state("phy"), Some(false));

	assert!(report.includes[0].found.is_some());
	assert!(report.includes[1].found.is_none());

	let material = |name: &str| report.materials.iter().find(|m| m.name == name).unwrap();
	let crate_ = material("Crate");
	assert_eq!(
		crate_.vmt.as_ref().unwrap().path,
		"materials/models/props/crate.vmt"
	);
	assert_eq!(crate_.shader.as_deref(), Some("VertexLitGeneric"));
	let textures: Vec<(&str, &str, bool)> = crate_
		.textures
		.iter()
		.map(|t| (t.param.as_str(), t.path.as_str(), t.found.is_some()))
		.collect();
	assert_eq!(
		textures,
		[
			("$basetexture", "materials/models/props/crate.vtf", true),
			("$bumpmap", "materials/models/props/crate_normal.vtf", true),
			(
				"$envmapmask",
				"materials/models/props/missing_mask.vtf",
				false
			),
		]
	);
	assert!(crate_.problems.is_empty());

	let lid = material("lid");
	assert_eq!(
		lid.tried,
		[
			"materials/models/props/lid.vmt",
			"materials/models/shared/lid.vmt"
		]
	);
	assert_eq!(
		lid.vmt.as_ref().unwrap().path,
		"materials/models/shared/lid.vmt"
	);
	assert_eq!(lid.textures.len(), 1);

	assert_eq!(
		material("world").problems,
		[MaterialProblem::WorldShader("LightmappedGeneric".into())]
	);
	let patched = material("patched");
	assert_eq!(patched.shader.as_deref(), Some("VertexLitGeneric"));
	assert_eq!(patched.textures[0].name, "models/props/other");
	assert_eq!(patched.textures.len(), 3);

	assert_eq!(material("gone").problems, [MaterialProblem::Missing]);
	let leftover = material("leftover");
	assert!(!leftover.used);
	assert_eq!(leftover.problems, [MaterialProblem::Missing]);

	// phy, include, world shader, missing material, 3 textures; unused ones don't count.
	assert_eq!(report.problem_count(), 7);
}

#[test]
fn a_file_on_disk_and_the_copy_the_game_loads() {
	let temp = tempfile::tempdir().unwrap();
	let work = temp.path().join("work/models/props");
	write(&work.join("Crate.mdl"), &crate_model());
	write(&work.join("CRATE.VVD"), &vvd(41));
	let game_dir = temp.path().join("game");
	write(&game_dir.join("models/props/crate.mdl"), &crate_model());

	let vfs = game(&[&game_dir]);
	let report = inspect(&vfs, &ModelSource::File(work.join("Crate.mdl"))).unwrap();
	assert_eq!(report.game_path.as_deref(), Some("models/props/crate.mdl"));
	assert_eq!(
		report.game_copy.as_ref().unwrap().file(),
		game_dir.join("models/props/crate.mdl")
	);
	let vvd = &report.companions[0];
	assert!(
		matches!(&vvd.state, CompanionState::Found { source, matches: false } if source.path == "CRATE.VVD")
	);
	assert!(matches!(
		report.companions[1].state,
		CompanionState::Missing
	));

	let same = inspect(
		&vfs,
		&ModelSource::File(game_dir.join("models/props/crate.mdl")),
	)
	.unwrap();
	assert!(same.game_copy.is_none());
}

#[test]
fn errors() {
	let temp = tempfile::tempdir().unwrap();
	write(&temp.path().join("models/bad.mdl"), b"nope");
	let vfs = game(&[temp.path()]);
	assert!(inspect(&vfs, &ModelSource::Game("models/none.mdl".into())).is_err());
	assert!(inspect(&vfs, &ModelSource::Game("models/bad.mdl".into())).is_err());
}

#[test]
fn texture_params() {
	assert!(is_texture_param("$basetexture", "a/b"));
	assert!(is_texture_param("$basetexture2", "a/b"));
	assert!(is_texture_param("$bumpmap", "a/b"));
	assert!(is_texture_param("$detail", "a/b"));
	assert!(is_texture_param("$envmap", "env/metal"));
	assert!(!is_texture_param("$envmap", "env_cubemap"));
	assert!(!is_texture_param("$basetexture", "_rt_FullFrameFB"));
	assert!(!is_texture_param("$basemapalphaphongmask", "1"));
	assert!(!is_texture_param("$envmaptint", "[1 1 1]"));
	assert!(!is_texture_param("basetexture", "a"));
	assert!(!is_texture_param("$surfaceprop", "metal"));
}

#[test]
fn material_paths_join_folder_and_name_as_is() {
	assert_eq!(
		material_paths("Foo.vmt", &["models\\A\\".into(), "".into()]),
		["materials/models/a/foo.vmt", "materials/foo.vmt"]
	);
	assert_eq!(material_paths("x/y", &[]), ["materials/x/y.vmt"]);
	assert_eq!(
		material_paths("b", &["models/nofolderslash".into()]),
		["materials/models/nofolderslashb.vmt"]
	);
}

#[test]
fn only_the_first_block_counts() {
	assert_eq!(first_block("a { b \"}\" } c { d"), "a { b \"}\" }");
	assert_eq!(first_block("a { // }\n b c"), "a { // }\n b c\n}");
}
