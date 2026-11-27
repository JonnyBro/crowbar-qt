//! The .mdl header, versions 44–49; the .vvd, .vtx and .phy readers are in submodules.

use std::fmt;

pub mod anim;
pub mod flex;
pub mod phy;
pub mod vtx;
pub mod vvd;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
	NotMdl,
	Version(i32),
	/// An offset or count points outside the file.
	Truncated,
	/// The animations live in a .ani that was not given.
	MissingAni,
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::NotMdl => f.write_str("not a compiled model (.mdl)"),
			Self::Version(v) => write!(f, "model version {v} is not supported (44 to 49 are)"),
			Self::Truncated => f.write_str("the model file is cut short or damaged"),
			Self::MissingAni => f.write_str("the animations are in a .ani file that was not found"),
		}
	}
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub const VERSIONS: std::ops::RangeInclusive<i32> = 44..=49;

/// `$staticprop`.
pub const FLAG_STATIC_PROP: u32 = 0x10;

pub const BONE_SCREEN_ALIGN_SPHERE: u32 = 0x08;
pub const BONE_SCREEN_ALIGN_CYLINDER: u32 = 0x10;
pub const BONE_USED_BY_BONE_MERGE: u32 = 0x40000;
pub const BONE_FIXED_ALIGNMENT: u32 = 0x100000;
pub const BONE_HAS_SAVEFRAME_POS: u32 = 0x200000;
pub const BONE_HAS_SAVEFRAME_ROT: u32 = 0x400000;
pub const ATTACHMENT_WORLD_ALIGN: u32 = 0x10000;

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
	pub version: i32,
	/// Must match the .vvd, .vtx and .phy, or the game rejects them.
	pub checksum: i32,
	pub name: String,
	pub flags: u32,
	pub eye_position: [f32; 3],
	pub illum_position: [f32; 3],
	pub hull_min: [f32; 3],
	pub hull_max: [f32; 3],
	pub view_min: [f32; 3],
	pub view_max: [f32; 3],
	pub mass: f32,
	pub contents: i32,
	pub directional_light_dot: u8,
	pub surface_prop: String,
	pub key_values: String,
	pub bones: Vec<Bone>,
	pub sequences: Vec<Sequence>,
	pub attachments: Vec<Attachment>,
	pub hitbox_sets: Vec<HitboxSet>,
	pub pose_parameters: Vec<String>,
	pub flex_count: usize,
	/// Without folder.
	pub materials: Vec<String>,
	/// `$cdmaterials`, tried in order.
	pub material_folders: Vec<String>,
	/// Per skin family: mesh skin slot to index in `materials`.
	pub skins: Vec<Vec<u16>>,
	pub body_parts: Vec<BodyPart>,
	pub include_models: Vec<String>,
	/// Animations kept in a separate .ani.
	pub anim_blocks: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
	pub name: String,
	/// -1 for a root.
	pub parent: i32,
	/// Bind pose, relative to the parent.
	pub position: [f32; 3],
	/// Radians.
	pub rotation: [f32; 3],
	/// Model space to bone space in the bind pose.
	pub pose_to_bone: [[f32; 4]; 3],
	pub flags: u32,
	pub surface_prop: String,
	pub contents: i32,
	pub jiggle: Option<Jiggle>,
	/// A helper bone driven by others, from `$proceduralbones`.
	pub procedural: Option<Procedural>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Procedural {
	/// Moves to the poses of whichever triggers the control bone's rotation is near.
	Quaternion {
		control: i32,
		triggers: Vec<Trigger>,
	},
	/// Six poses, one per direction the control bone's axis can point.
	Axis {
		control: i32,
		axis: i32,
		positions: [[f32; 3]; 6],
		rotations: [[f32; 4]; 6],
	},
	/// Points at another bone, or at an attachment when `at_attachment`.
	AimAt {
		parent: i32,
		aim: i32,
		at_attachment: bool,
		aim_vector: [f32; 3],
		up_vector: [f32; 3],
		base: [f32; 3],
	},
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trigger {
	/// Radians.
	pub tolerance: f32,
	pub trigger: [f32; 4],
	pub position: [f32; 3],
	pub rotation: [f32; 4],
}

pub const JIGGLE_FLEXIBLE: u32 = 0x01;
pub const JIGGLE_RIGID: u32 = 0x02;
pub const JIGGLE_YAW_CONSTRAINT: u32 = 0x04;
pub const JIGGLE_PITCH_CONSTRAINT: u32 = 0x08;
pub const JIGGLE_ANGLE_CONSTRAINT: u32 = 0x10;
pub const JIGGLE_LENGTH_CONSTRAINT: u32 = 0x20;
pub const JIGGLE_BASE_SPRING: u32 = 0x40;
pub const JIGGLE_BOING: u32 = 0x80;

/// `$jigglebone`; angles in radians.
#[derive(Debug, Clone, PartialEq)]
pub struct Jiggle {
	pub flags: u32,
	pub length: f32,
	pub tip_mass: f32,
	pub yaw_stiffness: f32,
	pub yaw_damping: f32,
	pub pitch_stiffness: f32,
	pub pitch_damping: f32,
	pub along_stiffness: f32,
	pub along_damping: f32,
	pub angle_limit: f32,
	pub yaw: [f32; 2],
	pub yaw_friction: f32,
	pub yaw_bounce: f32,
	pub pitch: [f32; 2],
	pub pitch_friction: f32,
	pub pitch_bounce: f32,
	pub base_mass: f32,
	pub base_stiffness: f32,
	pub base_damping: f32,
	/// Min, max and friction along left, up and forward.
	pub base_left: [f32; 3],
	pub base_up: [f32; 3],
	pub base_forward: [f32; 3],
	/// Impact speed, impact angle, damping rate, frequency, amplitude.
	pub boing: [f32; 5],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
	pub name: String,
	pub bone: i32,
	pub flags: u32,
	/// 3×4, relative to the bone.
	pub matrix: [[f32; 4]; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct HitboxSet {
	pub name: String,
	pub boxes: Vec<Hitbox>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hitbox {
	pub bone: i32,
	pub group: i32,
	pub min: [f32; 3],
	pub max: [f32; 3],
	pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
	pub name: String,
	pub activity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyPart {
	pub name: String,
	pub models: Vec<SubModel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubModel {
	/// Empty for a `blank` choice.
	pub name: String,
	pub vertices: u32,
	/// First vertex in the .vvd.
	pub first_vertex: u32,
	pub meshes: Vec<Mesh>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mesh {
	pub skin_slot: u16,
	/// Relative to the submodel's first vertex.
	pub first_vertex: u32,
	pub vertices: u32,
}

impl Model {
	/// Models often list materials no mesh uses.
	pub fn material_used(&self, index: usize) -> bool {
		let slots = self
			.body_parts
			.iter()
			.flat_map(|p| &p.models)
			.flat_map(|m| &m.meshes)
			.map(|mesh| mesh.skin_slot);
		slots.into_iter().any(|slot| {
			self.skins
				.iter()
				.any(|row| row.get(slot as usize).is_some_and(|&m| m as usize == index))
		})
	}

	/// Animation-only models have no geometry and need no .vvd or .vtx.
	pub fn has_geometry(&self) -> bool {
		self.body_parts
			.iter()
			.flat_map(|p| &p.models)
			.any(|m| m.vertices > 0)
	}

	pub fn is_static_prop(&self) -> bool {
		self.flags & FLAG_STATIC_PROP != 0
	}
}

const HEADER_SIZE: usize = 408;
const TEXTURE_SIZE: usize = 64;
const BODY_PART_SIZE: usize = 16;
const SUBMODEL_SIZE: usize = 148;
const MESH_SIZE: usize = 116;
const BONE_SIZE: usize = 216;
const SEQUENCE_SIZE: usize = 212;
const ATTACHMENT_SIZE: usize = 92;
const HITBOX_SET_SIZE: usize = 12;
const HITBOX_SIZE: usize = 68;
/// One `mstudiovertex_t` in the .vvd; submodels give their first vertex in bytes.
const VVD_VERTEX_SIZE: i32 = 48;
const POSE_PARAMETER_SIZE: usize = 20;
const PROC_AXIS: i32 = 1;
const PROC_QUATERNION: i32 = 2;
const PROC_AIM_AT_BONE: i32 = 3;
const PROC_AIM_AT_ATTACHMENT: i32 = 4;
const PROC_JIGGLE: i32 = 5;
const TRIGGER_SIZE: usize = 48;
const INCLUDE_MODEL_SIZE: usize = 8;

/// Tells an unsupported model from a broken one.
pub fn version(data: &[u8]) -> Result<i32> {
	if data.get(..4) != Some(b"IDST") {
		return Err(Error::NotMdl);
	}
	Reader(data).i32(4)
}

pub fn parse(data: &[u8]) -> Result<Model> {
	let version = version(data)?;
	if !VERSIONS.contains(&version) {
		return Err(Error::Version(version));
	}
	let r = Reader(data);
	if data.len() < HEADER_SIZE {
		return Err(Error::Truncated);
	}
	let vec3 = |at: usize| -> Result<[f32; 3]> { Ok([r.f32(at)?, r.f32(at + 4)?, r.f32(at + 8)?]) };
	let matrix = |at: usize| -> Result<[[f32; 4]; 3]> {
		let mut m = [[0.0; 4]; 3];
		for (row, cells) in m.iter_mut().enumerate() {
			for (col, cell) in cells.iter_mut().enumerate() {
				*cell = r.f32(at + (row * 4 + col) * 4)?;
			}
		}
		Ok(m)
	};

	let bones = r.table(156, BONE_SIZE, |at| {
		let kind = r.i32(at + 164)?;
		// A broken one is left out rather than failing the whole model.
		let data = at.checked_add_signed(r.i32(at + 168)? as isize);
		Ok(Bone {
			name: r.name_at(at, at)?,
			parent: r.i32(at + 4)?,
			position: vec3(at + 32)?,
			rotation: vec3(at + 60)?,
			pose_to_bone: matrix(at + 96)?,
			flags: r.i32(at + 160)? as u32,
			surface_prop: r.name_at(at, at + 176)?,
			contents: r.i32(at + 180)?,
			jiggle: data
				.filter(|_| kind == PROC_JIGGLE)
				.and_then(|j| jiggle(r, j).ok()),
			procedural: data.and_then(|p| procedural(r, kind, p).ok()).flatten(),
		})
	})?;
	let sequences = r.table(188, SEQUENCE_SIZE, |at| {
		Ok(Sequence {
			name: r.name_at(at, at + 4)?,
			activity: r.name_at(at, at + 8)?,
		})
	})?;
	let materials = r.table(204, TEXTURE_SIZE, |at| r.name_at(at, at))?;
	let material_folders = r.table(212, 4, |at| r.string(r.offset(at)?))?;

	let skin_slots = r.count(220)?;
	let skin_families = r.count(224)?;
	let skin_start = r.offset(228)?;
	let mut skins = Vec::with_capacity(skin_families.min(256));
	for family in 0..skin_families {
		let mut row = Vec::with_capacity(skin_slots.min(256));
		for slot in 0..skin_slots {
			row.push(r.u16(skin_start + (family * skin_slots + slot) * 2)?);
		}
		skins.push(row);
	}

	let body_parts = r.table(232, BODY_PART_SIZE, |part| {
		let name = r.name_at(part, part)?;
		let count = r.count(part + 4)?;
		let start = part + r.offset(part + 12)?;
		let mut models = Vec::with_capacity(count.min(256));
		for i in 0..count {
			let at = start + i * SUBMODEL_SIZE;
			let name = r.fixed_string(at, 64)?;
			let meshes = r.count(at + 72)?;
			let mesh_start = at + r.offset(at + 76)?;
			let vertices = r.i32(at + 80)?.max(0) as u32;
			let first_vertex = (r.i32(at + 84)? / VVD_VERTEX_SIZE).max(0) as u32;
			let meshes = (0..meshes)
				.map(|m| {
					let mesh = mesh_start + m * MESH_SIZE;
					Ok(Mesh {
						skin_slot: r.i32(mesh)?.clamp(0, u16::MAX as i32) as u16,
						vertices: r.i32(mesh + 8)?.max(0) as u32,
						first_vertex: r.i32(mesh + 12)?.max(0) as u32,
					})
				})
				.collect::<Result<_>>()?;
			models.push(SubModel {
				name,
				vertices,
				first_vertex,
				meshes,
			});
		}
		Ok(BodyPart { name, models })
	})?;

	let attachments = r.table(240, ATTACHMENT_SIZE, |at| {
		Ok(Attachment {
			name: r.name_at(at, at)?,
			bone: r.i32(at + 8)?,
			flags: r.i32(at + 4)? as u32,
			matrix: matrix(at + 12)?,
		})
	})?;
	let hitbox_sets = r.table(172, HITBOX_SET_SIZE, |set| {
		let count = r.count(set + 4)?;
		let start = set + r.offset(set + 8)?;
		let boxes = (0..count)
			.map(|i| {
				let at = start + i * HITBOX_SIZE;
				Ok(Hitbox {
					bone: r.i32(at)?,
					group: r.i32(at + 4)?,
					min: vec3(at + 8)?,
					max: vec3(at + 20)?,
					// Old studiomdl wrote this offset from the file start, at an empty string.
					name: r.name_at(at, at + 32).unwrap_or_default(),
				})
			})
			.collect::<Result<_>>()?;
		Ok(HitboxSet {
			name: r.name_at(set, set)?,
			boxes,
		})
	})?;
	let pose_parameters = r.table(300, POSE_PARAMETER_SIZE, |at| r.name_at(at, at))?;
	let include_models = r.table(336, INCLUDE_MODEL_SIZE, |at| r.name_at(at, at + 4))?;
	let key_values = match (r.offset(312)?, r.count(316)?) {
		(_, 0) => String::new(),
		(start, size) => {
			let bytes = data.get(start..start + size).ok_or(Error::Truncated)?;
			let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
			String::from_utf8_lossy(&bytes[..end]).into_owned()
		}
	};
	let surface_prop = match r.offset(308)? {
		0 => String::new(),
		at => r.string(at)?,
	};

	Ok(Model {
		version,
		checksum: r.i32(8)?,
		name: r.fixed_string(12, 64)?,
		flags: r.i32(152)? as u32,
		eye_position: vec3(80)?,
		illum_position: vec3(92)?,
		hull_min: vec3(104)?,
		hull_max: vec3(116)?,
		view_min: vec3(128)?,
		view_max: vec3(140)?,
		mass: r.f32(328)?,
		contents: r.i32(332)?,
		directional_light_dot: data[376],
		surface_prop,
		key_values,
		bones,
		sequences,
		attachments,
		hitbox_sets,
		pose_parameters,
		flex_count: r.count(260)?,
		materials,
		material_folders,
		skins,
		body_parts,
		include_models,
		anim_blocks: r.count(352)?,
	})
}

fn procedural(r: Reader, kind: i32, at: usize) -> Result<Option<Procedural>> {
	let vec3 = |at: usize| -> Result<[f32; 3]> { Ok([r.f32(at)?, r.f32(at + 4)?, r.f32(at + 8)?]) };
	let quat = |at: usize| -> Result<[f32; 4]> {
		Ok([r.f32(at)?, r.f32(at + 4)?, r.f32(at + 8)?, r.f32(at + 12)?])
	};
	Ok(Some(match kind {
		PROC_QUATERNION => {
			let count = r.count(at + 4)?;
			let start = at + r.offset(at + 8)?;
			let triggers = (0..count)
				.map(|i| {
					let t = start + i * TRIGGER_SIZE;
					let inverse = r.f32(t)?;
					Ok(Trigger {
						tolerance: if inverse != 0.0 { 1.0 / inverse } else { 0.0 },
						trigger: quat(t + 4)?,
						position: vec3(t + 20)?,
						rotation: quat(t + 32)?,
					})
				})
				.collect::<Result<_>>()?;
			Procedural::Quaternion {
				control: r.i32(at)?,
				triggers,
			}
		}
		PROC_AXIS => {
			let (mut positions, mut rotations) = ([[0.0; 3]; 6], [[0.0; 4]; 6]);
			for i in 0..6 {
				positions[i] = vec3(at + 8 + i * 12)?;
				rotations[i] = quat(at + 80 + i * 16)?;
			}
			Procedural::Axis {
				control: r.i32(at)?,
				axis: r.i32(at + 4)?,
				positions,
				rotations,
			}
		}
		PROC_AIM_AT_BONE | PROC_AIM_AT_ATTACHMENT => Procedural::AimAt {
			parent: r.i32(at)?,
			aim: r.i32(at + 4)?,
			at_attachment: kind == PROC_AIM_AT_ATTACHMENT,
			aim_vector: vec3(at + 8)?,
			up_vector: vec3(at + 20)?,
			base: vec3(at + 32)?,
		},
		_ => return Ok(None),
	}))
}

fn jiggle(r: Reader, at: usize) -> Result<Jiggle> {
	let f = |i: usize| r.f32(at + 4 + i * 4);
	let flags = r.i32(at)? as u32;
	Ok(Jiggle {
		flags,
		length: f(0)?,
		tip_mass: f(1)?,
		yaw_stiffness: f(2)?,
		yaw_damping: f(3)?,
		pitch_stiffness: f(4)?,
		pitch_damping: f(5)?,
		along_stiffness: f(6)?,
		along_damping: f(7)?,
		angle_limit: f(8)?,
		yaw: [f(9)?, f(10)?],
		yaw_friction: f(11)?,
		yaw_bounce: f(12)?,
		pitch: [f(13)?, f(14)?],
		pitch_friction: f(15)?,
		pitch_bounce: f(16)?,
		base_mass: f(17)?,
		base_stiffness: f(18)?,
		base_damping: f(19)?,
		base_left: [f(20)?, f(21)?, f(22)?],
		base_up: [f(23)?, f(24)?, f(25)?],
		base_forward: [f(26)?, f(27)?, f(28)?],
		// Older files end before these.
		boing: if flags & JIGGLE_BOING != 0 {
			[f(29)?, f(30)?, f(31)?, f(32)?, f(33)?]
		} else {
			[0.0; 5]
		},
	})
}

#[derive(Clone, Copy)]
pub(crate) struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
	pub(crate) fn bytes<const N: usize>(&self, at: usize) -> Result<[u8; N]> {
		self.0
			.get(at..at.checked_add(N).ok_or(Error::Truncated)?)
			.and_then(|b| b.try_into().ok())
			.ok_or(Error::Truncated)
	}

	pub(crate) fn i32(&self, at: usize) -> Result<i32> {
		Ok(i32::from_le_bytes(self.bytes(at)?))
	}

	pub(crate) fn u16(&self, at: usize) -> Result<u16> {
		Ok(u16::from_le_bytes(self.bytes(at)?))
	}

	pub(crate) fn f32(&self, at: usize) -> Result<f32> {
		Ok(f32::from_le_bytes(self.bytes(at)?))
	}

	/// Rejects negative counts and ones larger than the file.
	pub(crate) fn count(&self, at: usize) -> Result<usize> {
		let n = self.i32(at)?;
		usize::try_from(n)
			.ok()
			.filter(|&n| n <= self.0.len())
			.ok_or(Error::Truncated)
	}

	pub(crate) fn offset(&self, at: usize) -> Result<usize> {
		self.count(at)
	}

	/// Count at `at`, start offset right after it.
	fn table<T>(
		&self,
		at: usize,
		size: usize,
		mut read: impl FnMut(usize) -> Result<T>,
	) -> Result<Vec<T>> {
		let count = self.count(at)?;
		if count == 0 {
			return Ok(Vec::new());
		}
		let start = self.offset(at + 4)?;
		if start.saturating_add(count.saturating_mul(size)) > self.0.len() {
			return Err(Error::Truncated);
		}
		(0..count).map(|i| read(start + i * size)).collect()
	}

	/// Offset at `at` is relative to `base`.
	fn name_at(&self, base: usize, at: usize) -> Result<String> {
		let relative = self.i32(at)?;
		if relative == 0 {
			return Ok(String::new());
		}
		let start = (base as i64 + relative as i64)
			.try_into()
			.map_err(|_| Error::Truncated)?;
		self.string(start)
	}

	pub(crate) fn string(&self, at: usize) -> Result<String> {
		let rest = self.0.get(at..).ok_or(Error::Truncated)?;
		let end = rest.iter().position(|&b| b == 0).ok_or(Error::Truncated)?;
		Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
	}

	fn fixed_string(&self, at: usize, len: usize) -> Result<String> {
		let bytes = self.0.get(at..at + len).ok_or(Error::Truncated)?;
		let end = bytes.iter().position(|&b| b == 0).unwrap_or(len);
		Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
	}
}

pub fn companion_checksum(extension: &str, data: &[u8]) -> Option<i32> {
	let r = Reader(data);
	match extension {
		"vvd" => (data.get(..4) == Some(b"IDSV")).then(|| r.i32(8).ok())?,
		"phy" => r.i32(12).ok(),
		_ if extension.ends_with("vtx") => r.i32(16).ok(),
		_ => None,
	}
}

#[cfg(test)]
pub(crate) mod tests;
