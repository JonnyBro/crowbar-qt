use super::*;
use crate::mdl::tests::{Spec, build};
use crate::mdl::vtx::{self, Lod};
use crate::mdl::vvd;

fn vertex(position: [f32; 3], uv: [f32; 2]) -> Vertex {
	Vertex {
		weights: [1.0, 0.0, 0.0],
		bones: [0; 3],
		bone_count: 1,
		position,
		normal: [0.0, 0.0, 1.0],
		uv,
	}
}

fn vtx_of(checksum: i32, meshes_per_part: &[&[Vec<[u16; 3]>]]) -> Vtx {
	Vtx {
		checksum,
		body_parts: meshes_per_part
			.iter()
			.map(|models| {
				models
					.iter()
					.map(|triangles| vtx::Model {
						lods: vec![Lod {
							switch_point: 0.0,
							meshes: vec![vtx::Mesh {
								triangles: triangles.clone(),
							}],
						}],
					})
					.collect()
			})
			.collect(),
	}
}

fn file<'a>(files: &'a [File], path: &str) -> &'a str {
	&files.iter().find(|f| f.path == path).unwrap().text
}

#[test]
fn static_prop_reference_mesh() {
	let model = mdl::parse(&build(&Spec {
		checksum: 9,
		flags: mdl::FLAG_STATIC_PROP,
		bones: &["static_prop"],
		materials: &["wood"],
		folders: &["models/props/"],
		skins: &[&[0]],
		body_parts: &[("Body", &[("crate_ref.smd", 3, &[0])])],
		..Spec::default()
	}))
	.unwrap();
	let vvd = Vvd {
		checksum: 9,
		vertices: vec![
			vertex([1.0, 0.0, 0.0], [0.0, 0.25]),
			vertex([0.0, 2.0, 0.0], [1.0, 0.0]),
			vertex([0.0, 0.0, 3.0], [0.5, 1.0]),
		],
	};
	let files = decompile(&model, &vvd, &vtx_of(9, &[&[vec![[0, 1, 2]]]]), "crate").unwrap();
	let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
	assert_eq!(paths, ["crate_ref.smd", "crate_anims/idle.smd", "crate.qc"]);

	let smd = file(&files, "crate_ref.smd");
	let triangles = &smd[smd.find("triangles\n").unwrap()..];
	// Corners 0, 2, 1; X and Y turned back by 90°; V flipped.
	assert_eq!(
		triangles,
		"triangles\nwood\n\
		0 0.000000 -1.000000 0.000000 0.000000 0.000000 1.000000 0.000000 0.750000 1 0 1.000000\n\
		0 0.000000 0.000000 3.000000 0.000000 0.000000 1.000000 0.500000 0.000000 1 0 1.000000\n\
		0 2.000000 0.000000 0.000000 0.000000 0.000000 1.000000 1.000000 1.000000 1 0 1.000000\n\
		end\n"
	);

	let qc = file(&files, "crate.qc");
	assert!(qc.contains("$staticprop\n"));
	assert!(qc.contains("$body \"Body\" \"crate_ref.smd\"\n"));
	assert!(qc.contains("$cdmaterials \"models/props/\"\n"));
	assert!(qc.contains("$sequence \"idle\" \"crate_anims/idle.smd\"\n"));
	assert!(!qc.contains("$definebone"));
	assert!(!qc.contains("$texturegroup"));
}

#[test]
fn bodygroups_skins_bones_and_includes() {
	let model = mdl::parse(&build(&Spec {
		checksum: 1,
		bones: &["root", "hand"],
		materials: &["body", "head", "head_red", "body_alt"],
		skins: &[&[0, 1], &[0, 2], &[3, 2]],
		body_parts: &[
			("body", &[("body", 3, &[0])]),
			("hat", &[("hat on", 3, &[1]), ("", 0, &[])]),
		],
		includes: &["models\\anims.mdl"],
		attachments: &["muzzle"],
		..Spec::default()
	}))
	.unwrap();
	let vvd = Vvd {
		checksum: 1,
		vertices: vec![vertex([0.0; 3], [0.0; 2]); 3],
	};
	let tri = vec![[0, 1, 2]];
	let files = decompile(
		&model,
		&vvd,
		&vtx_of(1, &[std::slice::from_ref(&tri), &[tri.clone(), vec![]]]),
		"guy",
	)
	.unwrap();
	assert!(files.iter().any(|f| f.path == "hat_on.smd"));
	let qc = file(&files, "guy.qc");
	assert!(qc.contains("$bodygroup \"hat\"\n{\n\tstudio \"hat_on.smd\"\n\tblank\n}\n"));
	// Only the columns that change between families.
	assert!(qc.contains(
		"$texturegroup \"skinfamilies\"\n{\n\t{ \"body\" \"head\" }\n\t{ \"body\" \"head_red\" }\n\t{ \"body_alt\" \"head_red\" }\n}\n"
	));
	assert!(qc.contains("$includemodel \"anims.mdl\"\n"));
	assert!(qc.contains("$definebone \"root\" \"\" 0.000000 0.000000 0.000000"));
	assert!(qc.contains("$definebone \"hand\" \"root\" 1.000000 0.000000 0.000000"));
	assert!(qc.contains("$attachment \"muzzle\" \"root\" 0.000000 0.000000 0.000000 rotate"));

	// The bind pose with the root turned back for studiomdl.
	let idle = file(&files, "guy_anims/idle.smd");
	assert!(idle.contains("\n0 0.000000 0.000000 0.000000 0.000000 0.000000 -1.570796\n"));
	assert!(idle.contains("\n1 1.000000 0.000000 0.000000 0.000000 0.000000 0.000000\n"));
}

#[test]
fn other_builds_are_refused() {
	let model = mdl::parse(&build(&Spec {
		checksum: 1,
		..Spec::default()
	}))
	.unwrap();
	let vvd = Vvd {
		checksum: 2,
		vertices: Vec::new(),
	};
	assert!(decompile(&model, &vvd, &vtx_of(1, &[]), "x").is_err());
}

#[test]
fn attachment_angles_undo_angle_matrix() {
	let (p, y, r) = (20f32.to_radians(), -75f32.to_radians(), 130f32.to_radians());
	let (sp, cp, sy, cy, sr, cr) = (p.sin(), p.cos(), y.sin(), y.cos(), r.sin(), r.cos());
	let m = [
		[cp * cy, sr * sp * cy - cr * sy, cr * sp * cy + sr * sy, 0.0],
		[cp * sy, sr * sp * sy + cr * cy, cr * sp * sy - sr * cy, 0.0],
		[-sp, sr * cp, cr * cp, 0.0],
	];
	let angles = matrix_angles(m);
	for (got, want) in angles.iter().zip([20.0, -75.0, 130.0]) {
		assert!((got - want).abs() < 1e-3, "{angles:?}");
	}
}

#[test]
fn key_values_are_unwrapped() {
	let text = "mdlkeyvalue\n{\nprop_data {\n\"base\" \"Wooden.Medium\"  }\n}\n";
	assert_eq!(
		key_values(text).unwrap(),
		"\t\"prop_data\"\n\t{\n\t\t\"base\" \"Wooden.Medium\"\n\t}\n"
	);
	assert_eq!(key_values(""), None);
}

#[test]
fn names_and_numbers() {
	assert_eq!(num(-0.0000001), "0.000000");
	assert_eq!(num(-1.5), "-1.500000");
	assert_eq!(smd_name("arm l/ref.smd", "m"), "arm_l_ref");
	assert_eq!(smd_name("", "m"), "m");
	let mut used = HashSet::new();
	assert_eq!(unique(&mut used, "a"), "a.smd");
	assert_eq!(unique(&mut used, "A"), "A_2.smd");
}

fn vvd_file(vertices: usize, fixups: &[(i32, i32, i32)]) -> Vec<u8> {
	let mut d = b"IDSV".to_vec();
	let header = 64;
	let fixup_start = header;
	let vertex_start = fixup_start + fixups.len() * 12;
	for v in [
		4,
		5,
		1,
		vertices as i32,
		0,
		0,
		0,
		0,
		0,
		0,
		0,
		fixups.len() as i32,
		fixup_start as i32,
		vertex_start as i32,
		0,
	] {
		d.extend_from_slice(&v.to_le_bytes());
	}
	for (lod, source, count) in fixups {
		for v in [lod, source, count] {
			d.extend_from_slice(&v.to_le_bytes());
		}
	}
	for i in 0..vertices {
		let mut v = [0u8; 48];
		v[0..4].copy_from_slice(&1f32.to_le_bytes());
		v[15] = 1;
		v[16..20].copy_from_slice(&(i as f32).to_le_bytes());
		d.extend_from_slice(&v);
	}
	d
}

#[test]
fn vvd_follows_the_fixup_table() {
	let plain = vvd::parse(&vvd_file(3, &[])).unwrap();
	assert_eq!(plain.checksum, 5);
	assert_eq!(plain.vertices.len(), 3);
	assert_eq!(plain.vertices[2].position, [2.0, 0.0, 0.0]);
	assert_eq!(plain.vertices[0].bone_count, 1);

	let fixed = vvd::parse(&vvd_file(4, &[(1, 2, 2), (0, 0, 2)])).unwrap();
	let xs: Vec<f32> = fixed.vertices.iter().map(|v| v.position[0]).collect();
	assert_eq!(xs, [2.0, 3.0, 0.0, 1.0]);

	assert!(vvd::parse(&vvd_file(4, &[(0, 3, 5)])).is_err());
	assert!(vvd::parse(b"IDSV").is_err());
}

/// One mesh with one strip group; `extended` adds the CS:GO topology fields.
fn vtx_file(vertex_ids: &[u16], indices: &[u16], strip_flags: u8, extended: bool) -> Vec<u8> {
	let (group_size, strip_size) = if extended { (33, 35) } else { (25, 27) };
	let mut d = Vec::new();
	let i32s = |d: &mut Vec<u8>, vs: &[i32]| {
		for v in vs {
			d.extend_from_slice(&v.to_le_bytes());
		}
	};
	i32s(&mut d, &[7, 24]);
	d.extend_from_slice(&[0; 8]);
	i32s(&mut d, &[5, 1, 0, 1, 36]);
	i32s(&mut d, &[1, 8]); // body part
	i32s(&mut d, &[1, 8]); // model
	i32s(&mut d, &[1, 12]); // lod
	d.extend_from_slice(&0f32.to_le_bytes());
	i32s(&mut d, &[1, 9]); // mesh
	d.push(0);
	let vertices_at = group_size;
	let indices_at = vertices_at + vertex_ids.len() * 9;
	let strips_at = indices_at + indices.len() * 2;
	i32s(
		&mut d,
		&[
			vertex_ids.len() as i32,
			vertices_at as i32,
			indices.len() as i32,
			indices_at as i32,
			1,
			strips_at as i32,
		],
	);
	d.push(0);
	d.extend_from_slice(&vec![0; group_size - 25]);
	for id in vertex_ids {
		d.extend_from_slice(&[0, 0, 0, 1]);
		d.extend_from_slice(&id.to_le_bytes());
		d.extend_from_slice(&[0, 0, 0]);
	}
	for i in indices {
		d.extend_from_slice(&i.to_le_bytes());
	}
	i32s(
		&mut d,
		&[indices.len() as i32, 0, vertex_ids.len() as i32, 0],
	);
	d.extend_from_slice(&0i16.to_le_bytes());
	d.push(strip_flags);
	d.extend_from_slice(&vec![0; strip_size - 19]);
	d
}

#[test]
fn vtx_triangle_lists_and_strips() {
	let list = vtx::parse(&vtx_file(&[10, 11, 12, 13], &[0, 1, 2, 2, 1, 3], 1, false)).unwrap();
	assert_eq!(list.checksum, 5);
	let mesh = &list.body_parts[0][0].lods[0].meshes[0];
	assert_eq!(mesh.triangles, [[10, 11, 12], [12, 11, 13]]);

	// Every other strip triangle flips; the degenerate one is dropped.
	let strip = vtx::parse(&vtx_file(&[0, 1, 2, 3], &[0, 1, 2, 3, 3], 2, false)).unwrap();
	assert_eq!(
		strip.body_parts[0][0].lods[0].meshes[0].triangles,
		[[0, 1, 2], [2, 1, 3]]
	);

	let extended = vtx::parse(&vtx_file(&[5, 6, 7], &[0, 1, 2], 1, true)).unwrap();
	assert_eq!(
		extended.body_parts[0][0].lods[0].meshes[0].triangles,
		[[5, 6, 7]]
	);

	assert!(vtx::parse(&vtx_file(&[5], &[0, 1, 2], 1, false)).is_err());
}

#[test]
fn run_reads_the_files_beside_the_model() {
	let temp = tempfile::tempdir().unwrap();
	let dir = temp.path().join("in");
	std::fs::create_dir_all(&dir).unwrap();
	let mdl = build(&Spec {
		checksum: 5,
		bones: &["root"],
		materials: &["m"],
		skins: &[&[0]],
		body_parts: &[("body", &[("ref", 4, &[0])])],
		..Spec::default()
	});
	std::fs::write(dir.join("Thing.mdl"), mdl).unwrap();
	std::fs::write(dir.join("thing.vvd"), vvd_file(4, &[])).unwrap();
	std::fs::write(
		dir.join("thing.vtx"),
		vtx_file(&[0, 1, 2], &[0, 1, 2], 1, false),
	)
	.unwrap();

	let out = temp.path().join("out");
	let source = ModelSource::File(dir.join("Thing.mdl"));
	let written = run(None, &source, &out).unwrap();
	assert_eq!(written.len(), 3);
	assert!(out.join("Thing.qc").is_file());
	assert!(out.join("ref.smd").is_file());
	assert!(out.join("Thing_anims/idle.smd").is_file());

	std::fs::remove_file(dir.join("thing.vvd")).unwrap();
	assert!(run(None, &source, &out).unwrap_err().contains(".vvd"));
	let game = ModelSource::Game("models/thing.mdl".into());
	assert!(run(None, &game, &out).is_err());
}
