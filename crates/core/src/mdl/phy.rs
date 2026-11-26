//! `.phy`: convex pieces (IVP ledges) and a text block of masses and joints.

use super::{Error, Reader, Result};

/// IVP works in meters with Y and Z swapped.
const INCHES_PER_METER: f32 = 1.0 / 0.0254;
const SOLID_HEADER: usize = 28;
const SURFACE_HEADER: usize = 48;
const LEDGE_HEADER: usize = 16;
const TRIANGLE_SIZE: usize = 16;
const POINT_SIZE: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct Phy {
	pub checksum: i32,
	pub solids: Vec<Solid>,
	/// The key-values after the solids: `solid`, `ragdollconstraint`, `editparams`…
	pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Solid {
	pub pieces: Vec<Piece>,
}

/// One convex hull.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Piece {
	/// -1 when the piece names none.
	pub bone: i32,
	/// In the bone's space, inches.
	pub points: Vec<[f32; 3]>,
	pub triangles: Vec<[u16; 3]>,
}

pub fn parse(data: &[u8]) -> Result<Phy> {
	let r = Reader(data);
	let header = r.count(0)?;
	let count = r.count(8)?;
	let mut at = header;
	let mut solids = Vec::with_capacity(count.min(256));
	for _ in 0..count {
		let size = r.count(at)?;
		let start = at + 4;
		let end = start.checked_add(size).ok_or(Error::Truncated)?;
		if end > data.len() {
			return Err(Error::Truncated);
		}
		let surface = if data.get(start..start + 4) == Some(b"VPHY") {
			start + SOLID_HEADER
		} else {
			start
		};
		solids.push(solid(r, surface, end)?);
		at = end;
	}
	let rest = &data[at.min(data.len())..];
	let rest = &rest[..rest.iter().position(|&b| b == 0).unwrap_or(rest.len())];
	Ok(Phy {
		checksum: r.i32(12)?,
		solids,
		text: String::from_utf8_lossy(rest).into_owned(),
	})
}

fn solid(r: Reader, surface: usize, end: usize) -> Result<Solid> {
	if r.bytes::<4>(surface + 44)? != *b"IVPS" {
		return Err(Error::NotMdl);
	}
	// Ledges come first, then points; several ledges share one pool after the last.
	let mut ledges_end = (surface + r.count(surface + 32)?).min(end);
	let mut pieces = Vec::new();
	let mut ledge = surface + SURFACE_HEADER;
	while ledge + LEDGE_HEADER <= ledges_end {
		let points_at = ledge
			.checked_add_signed(r.i32(ledge)? as isize)
			.ok_or(Error::Truncated)?;
		ledges_end = ledges_end.min(points_at);
		let bone = r.i32(ledge + 4)? - 1;
		let has_children = r.i32(ledge + 8)? & 3 != 0;
		let count = r.u16(ledge + 12)? as usize;
		let next = ledge + LEDGE_HEADER + count * TRIANGLE_SIZE;
		// A ledge with children is the hull around the pieces below it, built by studiomdl.
		if !has_children {
			pieces.push(piece(r, ledge, points_at, bone, count)?);
		}
		ledge = next;
	}
	Ok(Solid { pieces })
}

fn piece(r: Reader, ledge: usize, points_at: usize, bone: i32, count: usize) -> Result<Piece> {
	let mut triangles = Vec::with_capacity(count);
	for t in 0..count {
		let at = ledge + LEDGE_HEADER + t * TRIANGLE_SIZE;
		triangles.push([r.u16(at + 4)?, r.u16(at + 8)?, r.u16(at + 12)?]);
	}
	// Indices point into the shared pool; only the ones used are kept.
	let mut used: Vec<u16> = triangles.iter().flatten().copied().collect();
	used.sort_unstable();
	used.dedup();
	let points = used
		.iter()
		.map(|&i| {
			let at = points_at + i as usize * POINT_SIZE;
			let (x, y, z) = (r.f32(at)?, r.f32(at + 4)?, r.f32(at + 8)?);
			Ok([x, z, -y].map(|v| v * INCHES_PER_METER))
		})
		.collect::<Result<_>>()?;
	let triangles = triangles
		.into_iter()
		.map(|t| t.map(|i| used.binary_search(&i).unwrap_or(0) as u16))
		.collect();
	Ok(Piece {
		bone,
		points,
		triangles,
	})
}
