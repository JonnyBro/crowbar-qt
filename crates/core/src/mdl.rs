//! Compiled model header, versions 44–49. Geometry (.vvd, .vtx) is not read.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
	NotMdl,
	Version(i32),
	/// An offset or count points outside the file.
	Truncated,
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::NotMdl => f.write_str("not a compiled model (.mdl)"),
			Self::Version(v) => write!(f, "model version {v} is not supported (44 to 49 are)"),
			Self::Truncated => f.write_str("the model file is cut short or damaged"),
		}
	}
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub const VERSIONS: std::ops::RangeInclusive<i32> = 44..=49;

/// `$staticprop`.
pub const FLAG_STATIC_PROP: u32 = 0x10;

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
	pub version: i32,
	/// Must match the .vvd, .vtx and .phy, or the game rejects them.
	pub checksum: i32,
	pub name: String,
	pub flags: u32,
	pub hull_min: [f32; 3],
	pub hull_max: [f32; 3],
	pub mass: f32,
	pub surface_prop: String,
	pub key_values: String,
	pub bones: Vec<String>,
	pub sequences: Vec<Sequence>,
	pub attachments: Vec<String>,
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
	pub mesh_skin_slots: Vec<u16>,
}

impl Model {
	/// Models often list materials no mesh uses.
	pub fn material_used(&self, index: usize) -> bool {
		let slots = self
			.body_parts
			.iter()
			.flat_map(|p| &p.models)
			.flat_map(|m| &m.mesh_skin_slots);
		slots.into_iter().any(|&slot| {
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

	pub fn materials_of(&self, submodel: &SubModel, skin: usize) -> Vec<usize> {
		let Some(row) = self.skins.get(skin).or_else(|| self.skins.first()) else {
			return Vec::new();
		};
		let mut out: Vec<usize> = submodel
			.mesh_skin_slots
			.iter()
			.filter_map(|&slot| row.get(slot as usize).map(|&m| m as usize))
			.collect();
		out.sort_unstable();
		out.dedup();
		out
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
const POSE_PARAMETER_SIZE: usize = 20;
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

	let bones = r.table(156, BONE_SIZE, |at| r.name_at(at, at))?;
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
			let mesh_skin_slots = (0..meshes)
				.map(|m| Ok(r.i32(mesh_start + m * MESH_SIZE)?.clamp(0, u16::MAX as i32) as u16))
				.collect::<Result<_>>()?;
			models.push(SubModel {
				name,
				vertices,
				mesh_skin_slots,
			});
		}
		Ok(BodyPart { name, models })
	})?;

	let attachments = r.table(240, ATTACHMENT_SIZE, |at| r.name_at(at, at))?;
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
		hull_min: vec3(104)?,
		hull_max: vec3(116)?,
		mass: r.f32(328)?,
		surface_prop,
		key_values,
		bones,
		sequences,
		attachments,
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

#[derive(Clone, Copy)]
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
	fn bytes<const N: usize>(&self, at: usize) -> Result<[u8; N]> {
		self.0
			.get(at..at.checked_add(N).ok_or(Error::Truncated)?)
			.and_then(|b| b.try_into().ok())
			.ok_or(Error::Truncated)
	}

	fn i32(&self, at: usize) -> Result<i32> {
		Ok(i32::from_le_bytes(self.bytes(at)?))
	}

	fn u16(&self, at: usize) -> Result<u16> {
		Ok(u16::from_le_bytes(self.bytes(at)?))
	}

	fn f32(&self, at: usize) -> Result<f32> {
		Ok(f32::from_le_bytes(self.bytes(at)?))
	}

	/// Rejects negative counts and ones larger than the file.
	fn count(&self, at: usize) -> Result<usize> {
		let n = self.i32(at)?;
		usize::try_from(n)
			.ok()
			.filter(|&n| n <= self.0.len())
			.ok_or(Error::Truncated)
	}

	fn offset(&self, at: usize) -> Result<usize> {
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

	fn string(&self, at: usize) -> Result<String> {
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
