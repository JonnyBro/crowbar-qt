use super::*;

/// A bodygroup choice: name, vertices, the skin slot of each mesh.
pub(crate) type Choice<'a> = (&'a str, i32, &'a [i32]);

/// The rest of the header stays zero.
#[derive(Default)]
pub(crate) struct Spec<'a> {
	pub checksum: i32,
	pub flags: u32,
	pub bones: &'a [&'a str],
	pub sequences: &'a [(&'a str, &'a str)],
	pub materials: &'a [&'a str],
	pub folders: &'a [&'a str],
	pub skins: &'a [&'a [u16]],
	pub body_parts: &'a [(&'a str, &'a [Choice<'a>])],
	pub attachments: &'a [&'a str],
	pub includes: &'a [&'a str],
	pub surface_prop: &'a str,
	/// Of every submodel, in .vvd vertices.
	pub first_vertex: i32,
}

struct Writer(Vec<u8>);

impl Writer {
	fn i32(&mut self, at: usize, v: i32) {
		self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
	}
	fn pos(&self) -> i32 {
		self.0.len() as i32
	}
	fn reserve(&mut self, size: usize) -> usize {
		let at = self.0.len();
		self.0.resize(at + size, 0);
		at
	}
	fn string(&mut self, s: &str) -> i32 {
		let at = self.pos();
		self.0.extend_from_slice(s.as_bytes());
		self.0.push(0);
		at
	}
	/// Name offsets go at `name_at`, relative to each record.
	fn named(&mut self, header: usize, size: usize, name_at: usize, names: &[&str]) {
		if names.is_empty() {
			return;
		}
		let start = self.reserve(size * names.len());
		self.i32(header, names.len() as i32);
		self.i32(header + 4, start as i32);
		for (i, name) in names.iter().enumerate() {
			let record = start + i * size;
			let at = self.string(name);
			self.i32(record + name_at, at - record as i32);
		}
	}
}

pub(crate) fn build(spec: &Spec) -> Vec<u8> {
	let mut w = Writer(vec![0; HEADER_SIZE]);
	w.0[..4].copy_from_slice(b"IDST");
	w.i32(4, 48);
	w.i32(8, spec.checksum);
	w.0[12..12 + 9].copy_from_slice(b"test.mdl\0");
	w.i32(152, spec.flags as i32);
	w.0[328..332].copy_from_slice(&5.5f32.to_le_bytes());

	w.named(156, BONE_SIZE, 0, spec.bones);
	if !spec.bones.is_empty() {
		// A chain: each bone hangs off the one before, one unit along X.
		let start = w.0[160..164].try_into().map(i32::from_le_bytes).unwrap() as usize;
		for i in 0..spec.bones.len() {
			let bone = start + i * BONE_SIZE;
			w.i32(bone + 4, i as i32 - 1);
			w.0[bone + 32..bone + 36].copy_from_slice(&(i as f32).to_le_bytes());
		}
	}
	let labels: Vec<&str> = spec.sequences.iter().map(|s| s.0).collect();
	w.named(188, SEQUENCE_SIZE, 4, &labels);
	if !spec.sequences.is_empty() {
		let start = w.0[192..196].try_into().map(i32::from_le_bytes).unwrap() as usize;
		for (i, (_, activity)) in spec.sequences.iter().enumerate() {
			let record = start + i * SEQUENCE_SIZE;
			let at = w.string(activity);
			w.i32(record + 8, at - record as i32);
		}
	}
	w.named(204, TEXTURE_SIZE, 0, spec.materials);
	if !spec.folders.is_empty() {
		let start = w.reserve(4 * spec.folders.len());
		w.i32(212, spec.folders.len() as i32);
		w.i32(216, start as i32);
		for (i, folder) in spec.folders.iter().enumerate() {
			let at = w.string(folder);
			w.i32(start + i * 4, at);
		}
	}
	if !spec.skins.is_empty() {
		let slots = spec.skins[0].len();
		w.i32(220, slots as i32);
		w.i32(224, spec.skins.len() as i32);
		w.i32(228, w.pos());
		for row in spec.skins {
			for slot in *row {
				w.0.extend_from_slice(&slot.to_le_bytes());
			}
		}
	}
	if !spec.body_parts.is_empty() {
		let parts = w.reserve(BODY_PART_SIZE * spec.body_parts.len());
		w.i32(232, spec.body_parts.len() as i32);
		w.i32(236, parts as i32);
		for (p, (name, models)) in spec.body_parts.iter().enumerate() {
			let part = parts + p * BODY_PART_SIZE;
			let at = w.string(name);
			w.i32(part, at - part as i32);
			w.i32(part + 4, models.len() as i32);
			let start = w.reserve(SUBMODEL_SIZE * models.len());
			w.i32(part + 12, (start - part) as i32);
			for (m, (name, vertices, meshes)) in models.iter().enumerate() {
				let model = start + m * SUBMODEL_SIZE;
				w.0[model..model + name.len()].copy_from_slice(name.as_bytes());
				w.i32(model + 72, meshes.len() as i32);
				w.i32(model + 80, *vertices);
				w.i32(model + 84, spec.first_vertex * VVD_VERTEX_SIZE);
				let mesh_start = w.reserve(MESH_SIZE * meshes.len());
				w.i32(model + 76, (mesh_start - model) as i32);
				for (i, slot) in meshes.iter().enumerate() {
					w.i32(mesh_start + i * MESH_SIZE, *slot);
				}
			}
		}
	}
	w.named(240, ATTACHMENT_SIZE, 0, spec.attachments);
	w.named(336, INCLUDE_MODEL_SIZE, 4, spec.includes);
	if !spec.surface_prop.is_empty() {
		let at = w.string(spec.surface_prop);
		w.i32(308, at);
	}
	w.0
}

#[test]
fn reads_what_a_model_needs() {
	let data = build(&Spec {
		checksum: 1234,
		flags: FLAG_STATIC_PROP,
		bones: &["root", "lid"],
		sequences: &[("idle", "ACT_IDLE")],
		materials: &["crate", "crate_dirty", "unused"],
		folders: &["models\\props\\", "models/shared/"],
		skins: &[&[0, 0], &[1, 1]],
		body_parts: &[
			("body", &[("crate_ref", 24, &[0])]),
			("lid", &[("lid_ref", 8, &[1]), ("", 0, &[])]),
		],
		attachments: &["top"],
		includes: &["models/anims.mdl"],
		surface_prop: "wood_crate",
		..Spec::default()
	});
	let m = parse(&data).unwrap();
	assert_eq!(m.version, 48);
	assert_eq!(m.checksum, 1234);
	assert_eq!(m.name, "test.mdl");
	assert!(m.is_static_prop());
	assert_eq!(m.mass, 5.5);
	let names: Vec<&str> = m.bones.iter().map(|b| b.name.as_str()).collect();
	assert_eq!(names, ["root", "lid"]);
	assert_eq!(m.bones[0].parent, -1);
	assert_eq!(
		m.sequences,
		[Sequence {
			name: "idle".into(),
			activity: "ACT_IDLE".into()
		}]
	);
	assert_eq!(m.materials, ["crate", "crate_dirty", "unused"]);
	assert_eq!(m.material_folders, ["models\\props\\", "models/shared/"]);
	assert_eq!(m.skins, [vec![0, 0], vec![1, 1]]);
	assert_eq!(m.attachments[0].name, "top");
	assert_eq!(m.include_models, ["models/anims.mdl"]);
	assert_eq!(m.surface_prop, "wood_crate");
	assert_eq!(m.body_parts.len(), 2);
	let lid = &m.body_parts[1];
	assert_eq!(lid.name, "lid");
	assert_eq!(lid.models[0].name, "lid_ref");
	assert_eq!(lid.models[0].vertices, 8);
	assert_eq!(lid.models[0].meshes[0].skin_slot, 1);
	assert_eq!(lid.models[1].name, "");

	assert_eq!(m.bones[1].parent, 0);
	assert_eq!(m.bones[1].position, [1.0, 0.0, 0.0]);
	assert!(m.material_used(0));
	assert!(m.material_used(1));
	assert!(!m.material_used(2));
	assert!(m.has_geometry());
}

#[test]
fn animation_only_models_have_no_geometry() {
	let m = parse(&build(&Spec {
		sequences: &[("walk", "")],
		..Spec::default()
	}))
	.unwrap();
	assert!(!m.has_geometry());
	assert!(m.body_parts.is_empty());
}

#[test]
fn errors() {
	assert_eq!(parse(b"VTF\0aaaa"), Err(Error::NotMdl));
	let mut data = build(&Spec::default());
	data[4] = 53;
	assert_eq!(parse(&data), Err(Error::Version(53)));

	let mut data = build(&Spec {
		materials: &["a"],
		..Spec::default()
	});
	// A material table that claims more entries than the file holds.
	data[204..208].copy_from_slice(&1000i32.to_le_bytes());
	assert_eq!(parse(&data), Err(Error::Truncated));
	assert_eq!(parse(&data[..100]), Err(Error::Truncated));
	data[204..208].copy_from_slice(&(-1i32).to_le_bytes());
	assert_eq!(parse(&data), Err(Error::Truncated));
}

#[test]
fn companion_checksums() {
	let mut vvd = b"IDSV".to_vec();
	vvd.extend_from_slice(&4i32.to_le_bytes());
	vvd.extend_from_slice(&77i32.to_le_bytes());
	assert_eq!(companion_checksum("vvd", &vvd), Some(77));
	assert_eq!(companion_checksum("vvd", b"XXXXaaaabbbb"), None);
	let mut vtx = vec![0u8; 16];
	vtx.extend_from_slice(&88i32.to_le_bytes());
	assert_eq!(companion_checksum("dx90.vtx", &vtx), Some(88));
	let mut phy = vec![0u8; 12];
	phy.extend_from_slice(&99i32.to_le_bytes());
	assert_eq!(companion_checksum("phy", &phy), Some(99));
}
