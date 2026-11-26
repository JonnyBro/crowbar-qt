//! `.vvd`: the model's vertices.

use super::{Error, Reader, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
	pub weights: [f32; 3],
	pub bones: [u8; 3],
	pub bone_count: u8,
	pub position: [f32; 3],
	pub normal: [f32; 3],
	pub uv: [f32; 2],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Vvd {
	pub checksum: i32,
	/// The vertices of LOD 0, in the order the .mdl counts them.
	pub vertices: Vec<Vertex>,
}

const HEADER_SIZE: usize = 64;
const VERTEX_SIZE: usize = 48;
const FIXUP_SIZE: usize = 12;

pub fn parse(data: &[u8]) -> Result<Vvd> {
	if data.get(..4) != Some(b"IDSV") {
		return Err(Error::NotMdl);
	}
	if data.len() < HEADER_SIZE {
		return Err(Error::Truncated);
	}
	let r = Reader(data);
	let count = r.count(16)?;
	let fixups = r.count(48)?;
	let fixup_start = r.offset(52)?;
	let start = r.offset(56)?;
	if start.saturating_add(count.saturating_mul(VERTEX_SIZE)) > data.len() {
		return Err(Error::Truncated);
	}
	let all = (0..count)
		.map(|i| vertex(r, start + i * VERTEX_SIZE))
		.collect::<Result<Vec<_>>>()?;
	if fixups == 0 {
		return Ok(Vvd {
			checksum: r.i32(8)?,
			vertices: all,
		});
	}
	// LOD fixups only drop vertices, so every entry belongs to LOD 0.
	let mut vertices = Vec::with_capacity(count);
	for i in 0..fixups {
		let at = fixup_start + i * FIXUP_SIZE;
		let source = r.count(at + 4)?;
		let len = r.count(at + 8)?;
		let slice = all
			.get(source..source.saturating_add(len))
			.ok_or(Error::Truncated)?;
		vertices.extend_from_slice(slice);
	}
	Ok(Vvd {
		checksum: r.i32(8)?,
		vertices,
	})
}

fn vertex(r: Reader, at: usize) -> Result<Vertex> {
	let f = |i: usize| r.f32(at + i * 4);
	let b: [u8; 4] = r.bytes(at + 12)?;
	Ok(Vertex {
		weights: [f(0)?, f(1)?, f(2)?],
		bones: [b[0], b[1], b[2]],
		bone_count: b[3],
		position: [f(4)?, f(5)?, f(6)?],
		normal: [f(7)?, f(8)?, f(9)?],
		uv: [f(10)?, f(11)?],
	})
}
