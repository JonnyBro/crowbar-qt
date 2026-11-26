//! Flexes (vertex animation of faces), their controllers and rules, mouths and eyeballs.

use super::anim::{half, table_at};
use super::{Reader, Result};

const FLEX_DESC_SIZE: usize = 4;
const CONTROLLER_SIZE: usize = 20;
const RULE_SIZE: usize = 12;
const OP_SIZE: usize = 8;
const MOUTH_SIZE: usize = 20;
const BODY_PART_SIZE: usize = 16;
const SUBMODEL_SIZE: usize = 148;
const MESH_SIZE: usize = 116;
const EYEBALL_SIZE: usize = 172;
const MESH_FLEX_SIZE: usize = 60;
const VERT_ANIM_SIZE: usize = 16;
const VERT_ANIM_WRINKLE_SIZE: usize = 18;

pub const OP_CONST: i32 = 1;
pub const OP_FETCH1: i32 = 2;
pub const OP_FETCH2: i32 = 3;
pub const OP_ADD: i32 = 4;
pub const OP_SUB: i32 = 5;
pub const OP_MUL: i32 = 6;
pub const OP_DIV: i32 = 7;
pub const OP_NEG: i32 = 8;
pub const OP_MAX: i32 = 13;
pub const OP_MIN: i32 = 14;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Flexes {
	pub descs: Vec<String>,
	pub controllers: Vec<Controller>,
	pub rules: Vec<Rule>,
	pub mouths: Vec<Mouth>,
	/// Body part, then submodel.
	pub eyeballs: Vec<Vec<Vec<Eyeball>>>,
	/// Body part, then submodel, then mesh.
	pub meshes: Vec<Vec<Vec<Vec<MeshFlex>>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Controller {
	pub kind: String,
	pub name: String,
	pub min: f32,
	pub max: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
	pub flex: i32,
	pub ops: Vec<Op>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Op {
	pub op: i32,
	/// A controller or flex index, or the bits of a constant.
	pub value: i32,
}

impl Op {
	pub fn constant(&self) -> f32 {
		f32::from_bits(self.value as u32)
	}
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mouth {
	pub bone: i32,
	pub forward: [f32; 3],
	pub flex: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Eyeball {
	pub name: String,
	pub bone: i32,
	/// In the bone's space.
	pub origin: [f32; 3],
	pub z_offset: f32,
	pub radius: f32,
	pub texture: i32,
	pub iris_scale: f32,
	/// Lowerer, neutral and raiser flexes of each lid, and their heights.
	pub upper: [i32; 3],
	pub lower: [i32; 3],
	pub upper_target: [f32; 3],
	pub lower_target: [f32; 3],
	pub upper_lid: i32,
	pub lower_lid: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeshFlex {
	pub desc: i32,
	/// The right half of a stereo flex; 0 when there is none.
	pub pair: i32,
	pub targets: [f32; 4],
	pub vertices: Vec<VertAnim>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VertAnim {
	/// Within the mesh.
	pub index: u16,
	/// How much of the left half of a stereo flex, 0 to 255.
	pub side: u8,
	pub delta: [f32; 3],
	pub normal_delta: [f32; 3],
}

pub fn parse(data: &[u8]) -> Result<Flexes> {
	let r = Reader(data);
	let vec3 = |at: usize| -> Result<[f32; 3]> { Ok([r.f32(at)?, r.f32(at + 4)?, r.f32(at + 8)?]) };
	let descs = r.table(260, FLEX_DESC_SIZE, |at| r.name_at(at, at))?;
	let controllers = r.table(268, CONTROLLER_SIZE, |at| {
		Ok(Controller {
			kind: r.name_at(at, at)?,
			name: r.name_at(at, at + 4)?,
			min: r.f32(at + 12)?,
			max: r.f32(at + 16)?,
		})
	})?;
	let rules = r.table(276, RULE_SIZE, |at| {
		let count = r.count(at + 4)?;
		let start = at + r.offset(at + 8)?;
		let ops = (0..count)
			.map(|i| {
				let o = start + i * OP_SIZE;
				Ok(Op {
					op: r.i32(o)?,
					value: r.i32(o + 4)?,
				})
			})
			.collect::<Result<_>>()?;
		Ok(Rule {
			flex: r.i32(at)?,
			ops,
		})
	})?;
	let mouths = r.table(292, MOUTH_SIZE, |at| {
		Ok(Mouth {
			bone: r.i32(at)?,
			forward: vec3(at + 4)?,
			flex: r.i32(at + 16)?,
		})
	})?;

	let mut eyeballs = Vec::new();
	let mut meshes = Vec::new();
	for part in r.table(232, BODY_PART_SIZE, Ok)? {
		let count = r.count(part + 4)?;
		let start = part + r.offset(part + 12)?;
		let mut part_eyes = Vec::with_capacity(count.min(256));
		let mut part_meshes = Vec::with_capacity(count.min(256));
		for m in 0..count {
			let model = start + m * SUBMODEL_SIZE;
			let eyes = sub_table(r, model, model + 100, EYEBALL_SIZE, |at| {
				let ints = |o: usize| -> Result<[i32; 3]> {
					Ok([r.i32(at + o)?, r.i32(at + o + 4)?, r.i32(at + o + 8)?])
				};
				Ok(Eyeball {
					name: r.name_at(at, at)?,
					bone: r.i32(at + 4)?,
					origin: vec3(at + 8)?,
					z_offset: r.f32(at + 20)?,
					radius: r.f32(at + 24)?,
					texture: r.i32(at + 52)?,
					iris_scale: r.f32(at + 60)?,
					upper: ints(68)?,
					lower: ints(80)?,
					upper_target: vec3(92)?,
					lower_target: vec3(104)?,
					upper_lid: r.i32(at + 116)?,
					lower_lid: r.i32(at + 120)?,
				})
			})?;
			let mesh_flexes = sub_table(r, model, model + 72, MESH_SIZE, |mesh| {
				sub_table(r, mesh, mesh + 16, MESH_FLEX_SIZE, |flex| {
					let wrinkle = r.0.get(flex + 32).copied() == Some(1);
					let size = if wrinkle {
						VERT_ANIM_WRINKLE_SIZE
					} else {
						VERT_ANIM_SIZE
					};
					let vertices = sub_table(r, flex, flex + 20, size, |v| {
						let halves = |o: usize| -> Result<[f32; 3]> {
							Ok([0, 1, 2].map(|i| half(r.u16(v + o + i * 2).unwrap_or(0))))
						};
						Ok(VertAnim {
							index: r.u16(v)?,
							side: r.bytes::<2>(v + 2)?[1],
							delta: halves(4)?,
							normal_delta: halves(10)?,
						})
					})?;
					Ok(MeshFlex {
						desc: r.i32(flex)?,
						pair: r.i32(flex + 28)?,
						targets: [
							r.f32(flex + 4)?,
							r.f32(flex + 8)?,
							r.f32(flex + 12)?,
							r.f32(flex + 16)?,
						],
						vertices,
					})
				})
			})?;
			part_eyes.push(eyes);
			part_meshes.push(mesh_flexes);
		}
		eyeballs.push(part_eyes);
		meshes.push(part_meshes);
	}
	Ok(Flexes {
		descs,
		controllers,
		rules,
		mouths,
		eyeballs,
		meshes,
	})
}

/// A count at `at` and an offset after it, relative to `base`.
fn sub_table<T>(
	r: Reader,
	base: usize,
	at: usize,
	size: usize,
	read: impl FnMut(usize) -> Result<T>,
) -> Result<Vec<T>> {
	table_at(r, base, at, size, read)
}

#[cfg(test)]
mod tests;
