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
		material_replacements: Vec::new(),
	}
}

fn input<'a>(mdl: &'a [u8], vvd: &'a Vvd, vtx: &'a Vtx) -> Input<'a> {
	Input {
		mdl,
		ani: None,
		vvd: Some(vvd),
		vtx: Some(vtx),
		phy: None,
	}
}

fn file<'a>(files: &'a [File], path: &str) -> &'a str {
	&files.iter().find(|f| f.path == path).unwrap().text
}

#[test]
fn static_prop_reference_mesh() {
	let data = build(&Spec {
		checksum: 9,
		flags: mdl::FLAG_STATIC_PROP,
		bones: &["static_prop"],
		materials: &["wood"],
		folders: &["models/props/"],
		skins: &[&[0]],
		body_parts: &[("Body", &[("crate_ref.smd", 3, &[0])])],
		..Spec::default()
	});
	let model = mdl::parse(&data).unwrap();
	let vvd = Vvd {
		checksum: 9,
		vertices: vec![
			vertex([1.0, 0.0, 0.0], [0.0, 0.25]),
			vertex([0.0, 2.0, 0.0], [1.0, 0.0]),
			vertex([0.0, 0.0, 3.0], [0.5, 1.0]),
		],
	};
	let vtx = vtx_of(9, &[&[vec![[0, 1, 2]]]]);
	let files = decompile(&model, &input(&data, &vvd, &vtx), "crate").unwrap();
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
	let data = build(&Spec {
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
	});
	let model = mdl::parse(&data).unwrap();
	let vvd = Vvd {
		checksum: 1,
		vertices: vec![vertex([0.0; 3], [0.0; 2]); 3],
	};
	let tri = vec![[0, 1, 2]];
	let vtx = vtx_of(1, &[std::slice::from_ref(&tri), &[tri.clone(), vec![]]]);
	let files = decompile(&model, &input(&data, &vvd, &vtx), "guy").unwrap();
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
	let data = build(&Spec {
		checksum: 1,
		bones: &["root"],
		materials: &["m"],
		skins: &[&[0]],
		body_parts: &[("body", &[("body", 3, &[0])])],
		..Spec::default()
	});
	let model = mdl::parse(&data).unwrap();
	let vvd = Vvd {
		checksum: 2,
		vertices: Vec::new(),
	};
	let vtx = vtx_of(1, &[]);
	assert!(decompile(&model, &input(&data, &vvd, &vtx), "x").is_err());
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

/// A tetrahedron ledge, a parent hull to skip, the point pool, then `text`.
fn phy_file(checksum: i32, client: i32, text: &str) -> Vec<u8> {
	let mut s = Vec::new();
	let i32s = |d: &mut Vec<u8>, vs: &[i32]| {
		for v in vs {
			d.extend_from_slice(&v.to_le_bytes());
		}
	};
	// Surface header: the ledge tree root is past both ledges and the points.
	let ledge0 = 48;
	let tris = [[0u16, 1, 2], [0, 2, 3], [0, 3, 1], [1, 3, 2]];
	let ledge1 = ledge0 + 16 + tris.len() * 16;
	let pool = ledge1 + 16 + 16;
	let root = pool + 4 * 16;
	s.extend_from_slice(&[0; 32]);
	i32s(&mut s, &[root as i32, 0, 0]);
	s.extend_from_slice(b"IVPS");
	for (at, children, client, count) in [(ledge0, 0, client, tris.len()), (ledge1, 1, 0, 1)] {
		i32s(
			&mut s,
			&[(pool - at) as i32, client, children | 4 | (8 << 8)],
		);
		s.extend_from_slice(&(count as u16).to_le_bytes());
		s.extend_from_slice(&[0, 0]);
		let list: &[[u16; 3]] = if children == 0 { &tris } else { &[[0, 1, 2]] };
		for t in list {
			s.extend_from_slice(&[0; 4]);
			for i in t {
				s.extend_from_slice(&i.to_le_bytes());
				s.extend_from_slice(&[0, 0]);
			}
		}
	}
	for p in [
		[0.0f32, 0.0, 0.0],
		[0.0254, 0.0, 0.0],
		[0.0, 0.0254, 0.0],
		[0.0, 0.0, 0.0254],
	] {
		for v in p {
			s.extend_from_slice(&v.to_le_bytes());
		}
		s.extend_from_slice(&[0; 4]);
	}
	let mut solid = b"VPHY".to_vec();
	solid.extend_from_slice(&[0; 24]);
	solid.extend_from_slice(&s);

	let mut d = Vec::new();
	i32s(&mut d, &[16, 0, 1, checksum, solid.len() as i32]);
	d.extend_from_slice(&solid);
	d.extend_from_slice(text.as_bytes());
	d.push(0);
	d
}

#[test]
fn phy_reads_ledges_and_text() {
	let phy = mdl::phy::parse(&phy_file(5, 3, "solid {\n\"index\" \"0\"\n}\n")).unwrap();
	assert_eq!(phy.checksum, 5);
	assert_eq!(phy.solids.len(), 1);
	// The parent hull is left out.
	assert_eq!(phy.solids[0].pieces.len(), 1);
	let piece = &phy.solids[0].pieces[0];
	assert_eq!(piece.bone, 2);
	assert_eq!(piece.triangles.len(), 4);
	// Meters to inches, IVP's Y and Z swapped back.
	let round = |p: [f32; 3]| p.map(|v| (v * 1000.0).round() / 1000.0);
	assert_eq!(round(piece.points[1]), [1.0, 0.0, 0.0]);
	assert_eq!(round(piece.points[2]), [0.0, 0.0, -1.0]);
	assert_eq!(round(piece.points[3]), [0.0, 1.0, 0.0]);
	assert_eq!(phy.text, "solid {\n\"index\" \"0\"\n}\n");
	assert!(mdl::phy::parse(&phy_file(5, 0, "")[..60]).is_err());
}

#[test]
fn collision_model_for_a_static_prop() {
	let model = mdl::parse(&build(&Spec {
		checksum: 5,
		flags: mdl::FLAG_STATIC_PROP,
		bones: &["static_prop"],
		..Spec::default()
	}))
	.unwrap();
	let text = "solid {\n\"index\" \"0\"\n\"name\" \"box\"\n\"mass\" \"30.0\"\n\"inertia\" \"1.0\"\n\"damping\" \"0.0\"\n\"rotdamping\" \"0.0\"\n}\n\
		editparams {\n\"rootname\" \" \"\n\"totalmass\" \"30.0\"\n\"concave\" \"1\"\n}\n\
		break {\"model\" \"gib1\" \"fadetime\" \"20\" }\n";
	let phy = mdl::phy::parse(&phy_file(5, 0, text)).unwrap();
	let (smd, q) = physics::physics(&model, &phy, "box");
	assert_eq!(smd.path, "box_physics.smd");
	assert_eq!(
		q,
		"$collisionmodel \"box_physics.smd\"\n{\n\t$mass 30.0\n\t$inertia 1.0\n\t$damping 0.0\n\t$rotdamping 0.0\n\t$concave\n\t$maxconvexpieces 1\n}\n\
		$collisiontext\n{\n\t\"break\"\n\t{\n\t\t\"model\" \"gib1\"\n\t\t\"fadetime\" \"20\"\n\t}\n}\n"
	);
	// Turned back by 90° like the rest of a static prop; one normal per point.
	assert!(smd.text.contains("\n0 0.000000 -1.000000 0.000000 "));
	let lines: Vec<&str> = smd
		.text
		.lines()
		.filter(|l| l.starts_with("0 1.000000 0.000000 0.000000"))
		.collect();
	assert!(lines.len() > 1 && lines.iter().all(|l| *l == lines[0]));
}

#[test]
fn collision_joints_for_a_ragdoll() {
	let model = mdl::parse(&build(&Spec {
		checksum: 5,
		bones: &["pelvis", "thigh"],
		..Spec::default()
	}))
	.unwrap();
	let text = "solid {\n\"index\" \"0\"\n\"name\" \"pelvis\"\n\"mass\" \"10\"\n\"volume\" \"100\"\n\"inertia\" \"2\"\n}\n\
		solid {\n\"index\" \"1\"\n\"name\" \"thigh\"\n\"mass\" \"30\"\n\"volume\" \"100\"\n\"inertia\" \"5\"\n}\n\
		ragdollconstraint {\n\"parent\" \"0\"\n\"child\" \"1\"\n\"xmin\" \"-10\"\n\"xmax\" \"20\"\n\"xfriction\" \"0.2\"\n}\n\
		collisionrules {\n\"collisionpair\" \"0,1\"\n\"collisionpair\" \"1,0\"\n}\n\
		animatedfriction {\n\"animfrictionmin\" \"1\"\n\"animfrictionmax\" \"2\"\n\"animfrictiontimein\" \"3\"\n\"animfrictiontimeout\" \"4\"\n\"animfrictiontimehold\" \"5\"\n}\n\
		editparams {\n\"rootname\" \"pelvis\"\n\"totalmass\" \"40\"\n}\n";
	let mut phy = mdl::phy::parse(&phy_file(5, 2, text)).unwrap();
	phy.solids.push(phy.solids[0].clone());
	let (_, q) = physics::physics(&model, &phy, "guy");
	assert!(q.starts_with("$collisionjoints \"guy_physics.smd\"\n{\n\t$mass 40\n"));
	assert!(q.contains("\t$rootbone \"pelvis\"\n"));
	assert!(
		q.contains("\t$jointmassbias \"pelvis\" 0.500000\n\t$jointmassbias \"thigh\" 1.500000\n")
	);
	assert!(q.contains("\t$jointinertia \"thigh\" 5\n"));
	// Friction is stored a fifth of what the QC says.
	assert!(q.contains("\t$jointconstrain \"thigh\" x limit -10 20 1.000000\n"));
	assert!(
		q.contains("\t$jointcollide \"thigh\" \"pelvis\"\n\t$jointcollide \"pelvis\" \"thigh\"\n")
	);
	assert!(q.contains("\t$animatedfriction 1 2 3 5 4\n"));
}

fn lod(switch_point: f32, triangles: Vec<[u16; 3]>) -> Lod {
	Lod {
		switch_point,
		meshes: vec![vtx::Mesh { triangles }],
	}
}

#[test]
fn lods_replace_remove_and_merge_bones() {
	let data = build(&Spec {
		checksum: 1,
		// The shadow LOD flag.
		flags: 0x40,
		bones: &["root", "hand", "finger"],
		materials: &["skin", "skin_lod"],
		skins: &[&[0]],
		body_parts: &[("body", &[("body", 4, &[0])]), ("hat", &[("hat", 3, &[0])])],
		..Spec::default()
	});
	let model = mdl::parse(&data).unwrap();
	let weighted = |position: [f32; 3], bone: u8| Vertex {
		bones: [bone, 0, 0],
		..vertex(position, [0.0; 2])
	};
	// 0-3 are LOD 0, with the last on the finger; 4 is that corner in LOD 1, on the hand.
	let vvd = Vvd {
		checksum: 1,
		vertices: vec![
			weighted([0.0, 0.0, 0.0], 0),
			weighted([1.0, 0.0, 0.0], 1),
			weighted([0.0, 1.0, 0.0], 1),
			weighted([1.0, 1.0, 0.0], 2),
			weighted([1.0, 1.0, 0.1], 1),
		],
	};
	let full = vec![[0, 1, 2], [1, 3, 2]];
	let body = vtx::Model {
		lods: vec![
			lod(0.0, full.clone()),
			lod(12.0, vec![[0, 1, 2], [1, 4, 2]]),
			lod(-1.0, vec![[0, 1, 2]]),
		],
	};
	let brim = vec![[0, 1, 2]];
	let hat = vtx::Model {
		lods: vec![lod(0.0, brim.clone()), lod(12.0, brim), lod(-1.0, vec![])],
	};
	let vtx = Vtx {
		checksum: 1,
		body_parts: vec![vec![body], vec![hat]],
		material_replacements: vec![vec![], vec![(0, "skin_lod".into())], vec![]],
	};
	let files = decompile(&model, &input(&data, &vvd, &vtx), "guy").unwrap();
	let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
	assert!(paths.contains(&"body_lod1.smd"));
	assert!(paths.contains(&"body_shadowlod.smd"));
	// The hat's LOD 1 is its LOD 0 again, so it gets no file of its own.
	assert!(!paths.iter().any(|p| p.starts_with("hat_lod")));
	let qc = file(&files, "guy.qc");
	assert!(qc.contains(
		"$lod 12.000000\n{\n\treplacemodel \"body.smd\" \"body_lod1.smd\"\n\treplacebone \"finger\" \"hand\"\n\treplacematerial \"skin\" \"skin_lod\"\n}\n"
	));
	assert!(qc.contains(
		"$shadowlod\n{\n\treplacemodel \"body.smd\" \"body_shadowlod.smd\"\n\tremovemodel \"hat.smd\"\n"
	));
}

#[test]
fn vtx_material_replacements_point_backwards() {
	let mut d = vtx_file(&[0, 1, 2], &[0, 1, 2], 1, false);
	d[20..24].copy_from_slice(&2i32.to_le_bytes());
	let name = d.len();
	d.extend_from_slice(b"skin_lod\0");
	let entry = d.len();
	d.extend_from_slice(&3u16.to_le_bytes());
	d.extend_from_slice(&((name as i32) - (entry as i32)).to_le_bytes());
	let list = d.len();
	d[24..28].copy_from_slice(&(list as i32).to_le_bytes());
	d.extend_from_slice(&0i32.to_le_bytes());
	d.extend_from_slice(&0i32.to_le_bytes());
	d.extend_from_slice(&1i32.to_le_bytes());
	d.extend_from_slice(&((entry as i32) - (list as i32 + 8)).to_le_bytes());
	let vtx = vtx::parse(&d).unwrap();
	assert_eq!(
		vtx.material_replacements,
		[vec![], vec![(3, "skin_lod".to_owned())]]
	);
}

#[test]
fn animations_and_sequences() {
	let data = crate::mdl::anim::tests::sample();
	let model = mdl::parse(&data).unwrap();
	let input = Input {
		mdl: &data,
		ani: None,
		vvd: None,
		vtx: None,
		phy: None,
	};
	let files = decompile(&model, &input, "guy").unwrap();
	let qc = file(&files, "guy.qc");
	assert!(qc.contains(
		"$sequence \"run\" {\n\t\"guy_anims/run.smd\"\n\tfps 30.000000\n\twalkframe 2 LX\n\
		\tactivity \"ACT_RUN\" 1\n\tloop\n\tfadeout 0.300000\n\t{ event 7 1 }\n}\n"
	));
	let smd = file(&files, "guy_anims/run.smd");
	// Half of the movement is back on the root, then the root is turned for studiomdl.
	assert!(smd.contains("time 1\n0 2.000000 -5.500000 3.000000 0.000000 0.000000 -1.570796\n"));
	assert!(smd.contains("time 2\n0 2.000000 -8.000000 3.000000 0.000000 0.000000 -1.570796\n"));
	assert!(smd.ends_with("1 1.000000 -2.000000 0.500000 0.000000 0.000000 0.000000\nend\n"));
}

#[test]
fn angles_stay_near_the_previous_frame() {
	let [x, y, z] = animation::closest_angles([0.0, 0.0, -3.1], [0.0, 0.0, 3.1]);
	assert_eq!([x, y], [0.0, 0.0]);
	assert!((z - (std::f32::consts::TAU - 3.1)).abs() < 1e-5);
	// The other set of angles for the same rotation.
	let pi = std::f32::consts::PI;
	let r = animation::closest_angles([pi - 0.1, pi - 0.2, pi - 0.3], [-0.1, 0.2, -0.3]);
	assert!(
		r.iter()
			.zip([-0.1, 0.2, -0.3])
			.all(|(a, b)| (a - b).abs() < 1e-5)
	);
}
