//! Reading Valve Texture Format files (7.0–7.5) into RGBA pixels for previews.

use rust_i18n::t;

const SIGNATURE: &[u8; 4] = b"VTF\0";
const FLAG_ENVMAP: u32 = 0x4000;
const RESOURCE_HIGH_RES: [u8; 3] = [0x30, 0, 0];
const NO_FORMAT: i32 = -1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
	#[error("{}", t!("vtf.not_vtf"))]
	NotVtf,
	#[error("{}", t!("vtf.version", version = format!("{}.{}", .0, .1)))]
	Version(u32, u32),
	#[error("{}", t!("vtf.truncated"))]
	Truncated,
	#[error("{}", t!("vtf.format", format = .0))]
	Format(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
	Rgba8888,
	Abgr8888,
	Rgb888,
	Bgr888,
	Rgb565,
	I8,
	Ia88,
	P8,
	A8,
	Rgb888Bluescreen,
	Bgr888Bluescreen,
	Argb8888,
	Bgra8888,
	Dxt1,
	Dxt3,
	Dxt5,
	Bgrx8888,
	Bgr565,
	Bgrx5551,
	Bgra4444,
	Dxt1OneBitAlpha,
	Bgra5551,
	Uv88,
	Uvwq8888,
	Rgba16161616F,
	Rgba16161616,
	Uvlx8888,
	R32F,
	Rgb323232F,
	Rgba32323232F,
	Ati2n,
	Ati1n,
	/// Depth and other GPU-only formats, which have no pixels to show.
	Other(i32),
}

impl Format {
	fn from_code(code: i32) -> Self {
		use Format::*;
		match code {
			0 => Rgba8888,
			1 => Abgr8888,
			2 => Rgb888,
			3 => Bgr888,
			4 => Rgb565,
			5 => I8,
			6 => Ia88,
			7 => P8,
			8 => A8,
			9 => Rgb888Bluescreen,
			10 => Bgr888Bluescreen,
			11 => Argb8888,
			12 => Bgra8888,
			13 => Dxt1,
			14 => Dxt3,
			15 => Dxt5,
			16 => Bgrx8888,
			17 => Bgr565,
			18 => Bgrx5551,
			19 => Bgra4444,
			20 => Dxt1OneBitAlpha,
			21 => Bgra5551,
			22 => Uv88,
			23 => Uvwq8888,
			24 => Rgba16161616F,
			25 => Rgba16161616,
			26 => Uvlx8888,
			27 => R32F,
			28 => Rgb323232F,
			29 => Rgba32323232F,
			37 => Ati2n,
			38 => Ati1n,
			other => Other(other),
		}
	}

	pub fn name(self) -> String {
		use Format::*;
		let name = match self {
			Rgba8888 => "RGBA8888",
			Abgr8888 => "ABGR8888",
			Rgb888 => "RGB888",
			Bgr888 => "BGR888",
			Rgb565 => "RGB565",
			I8 => "I8",
			Ia88 => "IA88",
			P8 => "P8",
			A8 => "A8",
			Rgb888Bluescreen => "RGB888_BLUESCREEN",
			Bgr888Bluescreen => "BGR888_BLUESCREEN",
			Argb8888 => "ARGB8888",
			Bgra8888 => "BGRA8888",
			Dxt1 => "DXT1",
			Dxt3 => "DXT3",
			Dxt5 => "DXT5",
			Bgrx8888 => "BGRX8888",
			Bgr565 => "BGR565",
			Bgrx5551 => "BGRX5551",
			Bgra4444 => "BGRA4444",
			Dxt1OneBitAlpha => "DXT1_ONEBITALPHA",
			Bgra5551 => "BGRA5551",
			Uv88 => "UV88",
			Uvwq8888 => "UVWQ8888",
			Rgba16161616F => "RGBA16161616F",
			Rgba16161616 => "RGBA16161616",
			Uvlx8888 => "UVLX8888",
			R32F => "R32F",
			Rgb323232F => "RGB323232F",
			Rgba32323232F => "RGBA32323232F",
			Ati2n => "ATI2N",
			Ati1n => "ATI1N",
			Other(code) => return format!("#{code}"),
		};
		name.to_owned()
	}

	/// Bytes in one `width`×`height` image, or `None` for formats with no known layout.
	fn image_size(self, width: u32, height: u32) -> Option<usize> {
		use Format::*;
		let (w, h) = (width as usize, height as usize);
		let blocks = w.div_ceil(4) * h.div_ceil(4);
		let pixels = w * h;
		Some(match self {
			Dxt1 | Dxt1OneBitAlpha | Ati1n => blocks * 8,
			Dxt3 | Dxt5 | Ati2n => blocks * 16,
			I8 | P8 | A8 => pixels,
			Rgb565 | Ia88 | Bgr565 | Bgrx5551 | Bgra4444 | Bgra5551 | Uv88 => pixels * 2,
			Rgb888 | Bgr888 | Rgb888Bluescreen | Bgr888Bluescreen => pixels * 3,
			Rgba8888 | Abgr8888 | Argb8888 | Bgra8888 | Bgrx8888 | Uvwq8888 | Uvlx8888 | R32F => {
				pixels * 4
			}
			Rgba16161616F | Rgba16161616 => pixels * 8,
			Rgb323232F => pixels * 12,
			Rgba32323232F => pixels * 16,
			Other(_) => return None,
		})
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
	pub version: (u32, u32),
	pub width: u32,
	pub height: u32,
	pub depth: u32,
	pub format: Format,
	pub flags: u32,
	pub frames: u32,
	pub faces: u32,
	pub mipmaps: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
	pub width: u32,
	pub height: u32,
	pub rgba: Vec<u8>,
}

struct Header {
	info: Info,
	/// Where the mipmap chain starts, smallest mipmap first.
	data_start: usize,
}

impl Texture {
	/// Fast to write rather than small: previews are thrown away right after.
	pub fn to_png(&self) -> Vec<u8> {
		use image::ImageEncoder;
		use image::codecs::png::{CompressionType, FilterType, PngEncoder};
		let mut png = Vec::new();
		let encoder =
			PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::NoFilter);
		// Writing into a Vec only fails on a size mismatch, which decode rules out.
		let _ = encoder.write_image(
			&self.rgba,
			self.width,
			self.height,
			image::ExtendedColorType::Rgba8,
		);
		png
	}
}

pub fn info(data: &[u8]) -> Result<Info> {
	Ok(header(data)?.info)
}

/// The first frame and face of the largest mipmap no wider or taller than
/// `max_side`, or the smallest one if none is.
pub fn decode(data: &[u8], max_side: u32) -> Result<(Info, Texture)> {
	let header = header(data)?;
	let info = &header.info;
	let level = (0..info.mipmaps)
		.find(|&level| {
			let (w, h, _) = mip_size(info, level);
			w.max(h) <= max_side
		})
		.unwrap_or(info.mipmaps - 1);

	let unsupported = || Error::Format(info.format.name());
	let mut offset = header.data_start;
	// Mipmaps are stored smallest first, so everything after `level` comes before it.
	for smaller in level + 1..info.mipmaps {
		let size = level_size(info, smaller).ok_or_else(unsupported)?;
		offset = offset.checked_add(size).ok_or(Error::Truncated)?;
	}
	let (width, height, _) = mip_size(info, level);
	let size = info
		.format
		.image_size(width, height)
		.ok_or_else(unsupported)?;
	let image = offset
		.checked_add(size)
		.and_then(|end| data.get(offset..end))
		.ok_or(Error::Truncated)?;
	let mut rgba = to_rgba(info.format, image, width, height).ok_or_else(unsupported)?;
	// HDR cubemaps and masks kept in alpha often have it zero everywhere,
	// which would leave nothing to look at.
	if rgba.iter().skip(3).step_by(4).all(|&a| a == 0) {
		rgba.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
	}
	Ok((
		header.info.clone(),
		Texture {
			width,
			height,
			rgba,
		},
	))
}

fn header(data: &[u8]) -> Result<Header> {
	let mut r = Reader { data, pos: 0 };
	if r.bytes(4)? != SIGNATURE {
		return Err(Error::NotVtf);
	}
	let major = r.u32()?;
	let minor = r.u32()?;
	if major != 7 || minor > 5 {
		return Err(Error::Version(major, minor));
	}
	let header_size = r.u32()? as usize;
	let width = u32::from(r.u16()?);
	let height = u32::from(r.u16()?);
	let flags = r.u32()?;
	let frames = u32::from(r.u16()?).max(1);
	let start_frame = r.u16()?;
	r.skip(4 + 12 + 4 + 4)?;
	let format = Format::from_code(r.i32()?);
	let mipmaps = u32::from(r.u8()?).max(1);
	let low_format = r.i32()?;
	let low_width = u32::from(r.u8()?);
	let low_height = u32::from(r.u8()?);
	let depth = if minor >= 2 {
		u32::from(r.u16()?).max(1)
	} else {
		1
	};
	if width == 0 || height == 0 {
		return Err(Error::Truncated);
	}

	let mut high_res = None;
	if minor >= 3 {
		r.skip(3)?;
		let count = r.u32()?;
		r.skip(8)?;
		for _ in 0..count {
			let tag = r.bytes(3)?;
			r.skip(1)?;
			let value = r.u32()? as usize;
			if tag == RESOURCE_HIGH_RES {
				high_res = Some(value);
			}
		}
	}
	let data_start = match high_res {
		Some(offset) => offset,
		None if minor >= 3 => return Err(Error::Truncated),
		None => {
			let low_res = if low_format == NO_FORMAT || low_width == 0 || low_height == 0 {
				0
			} else {
				Format::from_code(low_format)
					.image_size(low_width, low_height)
					.unwrap_or(0)
			};
			header_size + low_res
		}
	};

	let mut info = Info {
		version: (major, minor),
		width,
		height,
		depth,
		format,
		flags,
		frames,
		faces: 1,
		mipmaps,
	};
	if flags & FLAG_ENVMAP != 0 {
		// Before 7.5, cubemaps may carry a seventh face, the sphere map; the
		// spec says start_frame tells, but writers disagree, so the file size decides.
		let with_sphere = (1..5).contains(&minor) && start_frame != 0xffff;
		info.faces = if with_sphere { 7 } else { 6 };
		if with_sphere && chain_size(&info).is_some_and(|s| data_start + s > data.len()) {
			info.faces = 6;
		}
	}
	Ok(Header { info, data_start })
}

fn mip_size(info: &Info, level: u32) -> (u32, u32, u32) {
	(
		(info.width >> level).max(1),
		(info.height >> level).max(1),
		(info.depth >> level).max(1),
	)
}

/// One mipmap level across every frame, face and depth slice.
fn level_size(info: &Info, level: u32) -> Option<usize> {
	let (w, h, d) = mip_size(info, level);
	let copies = (info.frames as usize)
		.checked_mul(info.faces as usize)?
		.checked_mul(d as usize)?;
	info.format.image_size(w, h)?.checked_mul(copies)
}

fn chain_size(info: &Info) -> Option<usize> {
	(0..info.mipmaps).try_fold(0usize, |total, level| {
		total.checked_add(level_size(info, level)?)
	})
}

fn to_rgba(format: Format, src: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
	use Format::*;
	let (w, h) = (width as usize, height as usize);
	let mut out = vec![0u8; w * h * 4];
	match format {
		Dxt1 | Dxt1OneBitAlpha | Dxt3 | Dxt5 | Ati1n | Ati2n => {
			decode_blocks(format, src, w, h, &mut out);
			return Some(out);
		}
		_ => {}
	}
	let px = |f: &dyn Fn(&[u8]) -> [u8; 4], stride: usize, out: &mut [u8]| {
		for (dst, chunk) in out
			.as_chunks_mut::<4>()
			.0
			.iter_mut()
			.zip(src.chunks_exact(stride))
		{
			*dst = f(chunk);
		}
	};
	match format {
		Rgba8888 => px(&|p| [p[0], p[1], p[2], p[3]], 4, &mut out),
		Uvwq8888 => px(
			&|p| [signed(p[0]), signed(p[1]), signed(p[2]), signed(p[3])],
			4,
			&mut out,
		),
		Uvlx8888 => px(&|p| [signed(p[0]), signed(p[1]), p[2], p[3]], 4, &mut out),
		Abgr8888 => px(&|p| [p[3], p[2], p[1], p[0]], 4, &mut out),
		Argb8888 => px(&|p| [p[1], p[2], p[3], p[0]], 4, &mut out),
		Bgra8888 => px(&|p| [p[2], p[1], p[0], p[3]], 4, &mut out),
		Bgrx8888 => px(&|p| [p[2], p[1], p[0], 255], 4, &mut out),
		Rgb888 => px(&|p| [p[0], p[1], p[2], 255], 3, &mut out),
		Bgr888 => px(&|p| [p[2], p[1], p[0], 255], 3, &mut out),
		Rgb888Bluescreen => px(&|p| bluescreen(p[0], p[1], p[2]), 3, &mut out),
		Bgr888Bluescreen => px(&|p| bluescreen(p[2], p[1], p[0]), 3, &mut out),
		I8 => px(&|p| [p[0], p[0], p[0], 255], 1, &mut out),
		A8 => px(&|p| [255, 255, 255, p[0]], 1, &mut out),
		Ia88 => px(&|p| [p[0], p[0], p[0], p[1]], 2, &mut out),
		Uv88 => px(&|p| [signed(p[0]), signed(p[1]), 255, 255], 2, &mut out),
		Rgb565 => px(
			&|p| {
				let v = u16::from_le_bytes([p[0], p[1]]);
				[expand5(v), expand6(v >> 5), expand5(v >> 11), 255]
			},
			2,
			&mut out,
		),
		Bgr565 => px(
			&|p| {
				let v = u16::from_le_bytes([p[0], p[1]]);
				[expand5(v >> 11), expand6(v >> 5), expand5(v), 255]
			},
			2,
			&mut out,
		),
		Bgrx5551 | Bgra5551 => {
			let alpha = format == Bgra5551;
			px(
				&|p| {
					let v = u16::from_le_bytes([p[0], p[1]]);
					let a = if !alpha || v & 0x8000 != 0 { 255 } else { 0 };
					[expand5(v >> 10), expand5(v >> 5), expand5(v), a]
				},
				2,
				&mut out,
			)
		}
		Bgra4444 => px(
			&|p| {
				let v = u16::from_le_bytes([p[0], p[1]]);
				let n = |s: u16| ((v >> s) & 0xf) as u8 * 17;
				[n(8), n(4), n(0), n(12)]
			},
			2,
			&mut out,
		),
		Rgba16161616 => px(&|p| [p[1], p[3], p[5], p[7]], 8, &mut out),
		Rgba16161616F => px(
			&|p| {
				let c = |i: usize| f16_to_f32(u16::from_le_bytes([p[i], p[i + 1]]));
				[tone(c(0)), tone(c(2)), tone(c(4)), unit(c(6))]
			},
			8,
			&mut out,
		),
		R32F => px(
			&|p| {
				let v = tone(f32_at(p, 0));
				[v, v, v, 255]
			},
			4,
			&mut out,
		),
		Rgb323232F => px(
			&|p| {
				[
					tone(f32_at(p, 0)),
					tone(f32_at(p, 4)),
					tone(f32_at(p, 8)),
					255,
				]
			},
			12,
			&mut out,
		),
		Rgba32323232F => px(
			&|p| {
				[
					tone(f32_at(p, 0)),
					tone(f32_at(p, 4)),
					tone(f32_at(p, 8)),
					unit(f32_at(p, 12)),
				]
			},
			16,
			&mut out,
		),
		P8 | Other(_) => return None,
		Dxt1 | Dxt1OneBitAlpha | Dxt3 | Dxt5 | Ati1n | Ati2n => unreachable!(),
	}
	Some(out)
}

fn bluescreen(r: u8, g: u8, b: u8) -> [u8; 4] {
	if (r, g, b) == (0, 0, 255) {
		[0, 0, 0, 0]
	} else {
		[r, g, b, 255]
	}
}

/// Du/dv maps store signed bytes; shifted, zero lands in the middle gray.
fn signed(v: u8) -> u8 {
	v ^ 0x80
}

fn expand5(v: u16) -> u8 {
	let v = (v & 0x1f) as u8;
	(v << 3) | (v >> 2)
}

fn expand6(v: u16) -> u8 {
	let v = (v & 0x3f) as u8;
	(v << 2) | (v >> 4)
}

fn f32_at(p: &[u8], at: usize) -> f32 {
	f32::from_le_bytes([p[at], p[at + 1], p[at + 2], p[at + 3]])
}

fn f16_to_f32(h: u16) -> f32 {
	let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
	let exp = i32::from((h >> 10) & 0x1f);
	let mantissa = f32::from(h & 0x3ff);
	match exp {
		0 => sign * mantissa * 2f32.powi(-24),
		31 => sign * f32::INFINITY,
		_ => sign * (1.0 + mantissa / 1024.0) * 2f32.powi(exp - 15),
	}
}

fn unit(v: f32) -> u8 {
	(v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// HDR texels are linear light; clamped and gamma-corrected they look about
/// like the game shows them at default exposure.
fn tone(v: f32) -> u8 {
	unit(v.max(0.0).powf(1.0 / 2.2))
}

fn decode_blocks(format: Format, src: &[u8], w: usize, h: usize, out: &mut [u8]) {
	let block_bytes = match format {
		Format::Dxt1 | Format::Dxt1OneBitAlpha | Format::Ati1n => 8,
		_ => 16,
	};
	let blocks_x = w.div_ceil(4);
	for (index, block) in src.chunks_exact(block_bytes).enumerate() {
		let (bx, by) = (index % blocks_x, index / blocks_x);
		let mut texels = [[0u8; 4]; 16];
		match format {
			Format::Dxt1 | Format::Dxt1OneBitAlpha => color_block(block, true, &mut texels),
			Format::Dxt3 => {
				color_block(&block[8..], false, &mut texels);
				for (i, t) in texels.iter_mut().enumerate() {
					let nibble = (block[i / 2] >> ((i % 2) * 4)) & 0xf;
					t[3] = nibble * 17;
				}
			}
			Format::Dxt5 => {
				color_block(&block[8..], false, &mut texels);
				let alpha = channel_block(block);
				for (t, a) in texels.iter_mut().zip(alpha) {
					t[3] = a;
				}
			}
			Format::Ati1n => {
				for (t, v) in texels.iter_mut().zip(channel_block(block)) {
					*t = [v, v, v, 255];
				}
			}
			Format::Ati2n => {
				let red = channel_block(&block[..8]);
				let green = channel_block(&block[8..]);
				for (i, t) in texels.iter_mut().enumerate() {
					let x = f32::from(red[i]) / 127.5 - 1.0;
					let y = f32::from(green[i]) / 127.5 - 1.0;
					let z = (1.0 - x * x - y * y).max(0.0).sqrt();
					*t = [red[i], green[i], ((z + 1.0) * 127.5) as u8, 255];
				}
			}
			_ => return,
		}
		for (i, texel) in texels.iter().enumerate() {
			let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
			if x < w && y < h {
				let at = (y * w + x) * 4;
				out[at..at + 4].copy_from_slice(texel);
			}
		}
	}
}

fn color_block(block: &[u8], one_bit_alpha: bool, texels: &mut [[u8; 4]; 16]) {
	let c0 = u16::from_le_bytes([block[0], block[1]]);
	let c1 = u16::from_le_bytes([block[2], block[3]]);
	let rgb = |c: u16| [expand5(c >> 11), expand6(c >> 5), expand5(c)];
	let (a, b) = (rgb(c0), rgb(c1));
	let mix = |wa: u16, wb: u16, div: u16| -> [u8; 4] {
		let m = |i: usize| ((u16::from(a[i]) * wa + u16::from(b[i]) * wb) / div) as u8;
		[m(0), m(1), m(2), 255]
	};
	// DXT3 and DXT5 always use four colors; only DXT1 has the
	// three-colors-and-transparent mode.
	let palette = if c0 > c1 || !one_bit_alpha {
		[
			[a[0], a[1], a[2], 255],
			[b[0], b[1], b[2], 255],
			mix(2, 1, 3),
			mix(1, 2, 3),
		]
	} else {
		[
			[a[0], a[1], a[2], 255],
			[b[0], b[1], b[2], 255],
			mix(1, 1, 2),
			[0, 0, 0, 0],
		]
	};
	let bits = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
	for (i, t) in texels.iter_mut().enumerate() {
		*t = palette[((bits >> (i * 2)) & 3) as usize];
	}
}

/// A BC4 block: the DXT5 alpha block, also used per channel by ATI1N and ATI2N.
fn channel_block(block: &[u8]) -> [u8; 16] {
	let (a0, a1) = (u16::from(block[0]), u16::from(block[1]));
	let mut palette = [0u8; 8];
	palette[0] = a0 as u8;
	palette[1] = a1 as u8;
	if a0 > a1 {
		for i in 1..7u16 {
			palette[i as usize + 1] = ((a0 * (7 - i) + a1 * i) / 7) as u8;
		}
	} else {
		for i in 1..5u16 {
			palette[i as usize + 1] = ((a0 * (5 - i) + a1 * i) / 5) as u8;
		}
		palette[6] = 0;
		palette[7] = 255;
	}
	let mut bits = 0u64;
	for (i, &b) in block[2..8].iter().enumerate() {
		bits |= u64::from(b) << (i * 8);
	}
	let mut out = [0u8; 16];
	for (i, v) in out.iter_mut().enumerate() {
		*v = palette[((bits >> (i * 3)) & 7) as usize];
	}
	out
}

struct Reader<'a> {
	data: &'a [u8],
	pos: usize,
}

impl<'a> Reader<'a> {
	fn bytes(&mut self, count: usize) -> Result<&'a [u8]> {
		let slice = self
			.data
			.get(self.pos..self.pos.saturating_add(count))
			.ok_or(Error::Truncated)?;
		self.pos += count;
		Ok(slice)
	}

	fn skip(&mut self, count: usize) -> Result<()> {
		self.bytes(count).map(|_| ())
	}

	fn u8(&mut self) -> Result<u8> {
		Ok(self.bytes(1)?[0])
	}

	fn u16(&mut self) -> Result<u16> {
		let b = self.bytes(2)?;
		Ok(u16::from_le_bytes([b[0], b[1]]))
	}

	fn u32(&mut self) -> Result<u32> {
		let b = self.bytes(4)?;
		Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
	}

	fn i32(&mut self) -> Result<i32> {
		Ok(self.u32()? as i32)
	}
}

#[cfg(test)]
mod tests;
