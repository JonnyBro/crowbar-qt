//! `.vtx`: triangles of each mesh, per LOD, indexing the .vvd vertices.

use super::{Error, Reader, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Vtx {
	pub checksum: i32,
	/// Body part, then submodel, as in the .mdl.
	pub body_parts: Vec<Vec<Model>>,
	/// Per LOD: the material index and the material drawn instead.
	pub material_replacements: Vec<Vec<(u16, String)>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
	pub lods: Vec<Lod>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lod {
	pub switch_point: f32,
	pub meshes: Vec<Mesh>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
	/// Each corner as a vertex of the .mdl mesh, in file order.
	pub triangles: Vec<[u16; 3]>,
}

const HEADER_SIZE: usize = 36;
const VERTEX_SIZE: usize = 9;
const STRIP_TRIANGLE_LIST: u8 = 1;
const STRIP_TRIANGLE_STRIP: u8 = 2;

/// CS:GO-era files add topology fields to strip groups and strips.
#[derive(Clone, Copy)]
struct Layout {
	strip_group: usize,
	strip: usize,
}

const LAYOUTS: [Layout; 2] = [
	Layout {
		strip_group: 25,
		strip: 27,
	},
	Layout {
		strip_group: 33,
		strip: 35,
	},
];

pub fn parse(data: &[u8]) -> Result<Vtx> {
	if data.len() < HEADER_SIZE {
		return Err(Error::Truncated);
	}
	let r = Reader(data);
	if r.i32(0)? != 7 {
		return Err(Error::NotMdl);
	}
	let mut last = Error::Truncated;
	for layout in LAYOUTS {
		match read(r, layout) {
			Ok(body_parts) => {
				return Ok(Vtx {
					checksum: r.i32(16)?,
					body_parts,
					material_replacements: replacements(r)?,
				});
			}
			Err(e) => last = e,
		}
	}
	Err(last)
}

fn replacements(r: Reader) -> Result<Vec<Vec<(u16, String)>>> {
	let lods = r.count(20)?;
	let start = r.offset(24)?;
	if start == 0 {
		return Ok(Vec::new());
	}
	// Offsets here point backwards: the names and entries come before the list.
	let signed = |base: usize, at: usize| {
		base.checked_add_signed(r.i32(at)? as isize)
			.ok_or(Error::Truncated)
	};
	(0..lods)
		.map(|lod| {
			let list = start + lod * 8;
			let first = signed(list, list + 4)?;
			(0..r.count(list)?)
				.map(|i| {
					let at = first + i * 6;
					Ok((r.u16(at)?, r.string(signed(at, at + 2)?)?))
				})
				.collect()
		})
		.collect()
}

fn read(r: Reader, layout: Layout) -> Result<Vec<Vec<Model>>> {
	table(r, 0, 28, 8, |part| {
		table(r, part, 0, 8, |model| {
			let lods = table(r, model, 0, 12, |lod| {
				Ok(Lod {
					switch_point: r.f32(lod + 8)?,
					meshes: table(r, lod, 0, 9, |mesh| {
						let mut out = Mesh::default();
						for group in table(r, mesh, 0, layout.strip_group, Ok)? {
							strip_group(r, group, layout, &mut out.triangles)?;
						}
						Ok(out)
					})?,
				})
			})?;
			Ok(Model { lods })
		})
	})
}

/// A count at `base + at` and the offset of the first record, relative to `base`, after it.
fn table<T>(
	r: Reader,
	base: usize,
	at: usize,
	size: usize,
	mut read: impl FnMut(usize) -> Result<T>,
) -> Result<Vec<T>> {
	let count = r.count(base + at)?;
	if count == 0 {
		return Ok(Vec::new());
	}
	let start = base + r.offset(base + at + 4)?;
	if start.saturating_add(count.saturating_mul(size)) > r.0.len() {
		return Err(Error::Truncated);
	}
	(0..count).map(|i| read(start + i * size)).collect()
}

fn strip_group(
	r: Reader,
	group: usize,
	layout: Layout,
	triangles: &mut Vec<[u16; 3]>,
) -> Result<()> {
	let vertices = table(r, group, 0, VERTEX_SIZE, |at| r.u16(at + 4))?;
	let indices = table(r, group, 8, 2, |at| r.u16(at))?;
	let corner = |i: usize| -> Result<u16> {
		let vertex = *indices.get(i).ok_or(Error::Truncated)?;
		vertices
			.get(vertex as usize)
			.copied()
			.ok_or(Error::Truncated)
	};
	let strips = table(r, group, 16, layout.strip, |at| {
		Ok((r.count(at)?, r.count(at + 4)?, r.bytes::<1>(at + 18)?[0]))
	})?;
	for (count, first, flags) in strips {
		if flags & STRIP_TRIANGLE_STRIP != 0 {
			for i in 0..count.saturating_sub(2) {
				let (a, b, c) = (first + i, first + i + 1, first + i + 2);
				let tri = if i % 2 == 0 { [a, b, c] } else { [b, a, c] };
				let tri = [corner(tri[0])?, corner(tri[1])?, corner(tri[2])?];
				if tri[0] != tri[1] && tri[1] != tri[2] && tri[0] != tri[2] {
					triangles.push(tri);
				}
			}
		} else if flags & STRIP_TRIANGLE_LIST != 0 || flags == 0 {
			for i in (0..count / 3).map(|t| first + t * 3) {
				triangles.push([corner(i)?, corner(i + 1)?, corner(i + 2)?]);
			}
		}
	}
	Ok(())
}
