use super::*;

struct Spec {
	minor: u32,
	width: u16,
	height: u16,
	format: i32,
	mipmaps: u8,
	flags: u32,
	frames: u16,
	start_frame: u16,
}

impl Spec {
	fn new(minor: u32, width: u16, height: u16, format: i32) -> Self {
		Self {
			minor,
			width,
			height,
			format,
			mipmaps: 1,
			flags: 0,
			frames: 1,
			start_frame: 0,
		}
	}
}

/// A file with no low-res thumbnail and `body` as the image data.
fn vtf(spec: &Spec, body: &[u8]) -> Vec<u8> {
	let header_size: u32 = if spec.minor >= 3 { 80 + 8 } else { 80 };
	let mut d = Vec::new();
	d.extend_from_slice(b"VTF\0");
	d.extend_from_slice(&7u32.to_le_bytes());
	d.extend_from_slice(&spec.minor.to_le_bytes());
	d.extend_from_slice(&header_size.to_le_bytes());
	d.extend_from_slice(&spec.width.to_le_bytes());
	d.extend_from_slice(&spec.height.to_le_bytes());
	d.extend_from_slice(&spec.flags.to_le_bytes());
	d.extend_from_slice(&spec.frames.to_le_bytes());
	d.extend_from_slice(&spec.start_frame.to_le_bytes());
	d.extend_from_slice(&[0; 4 + 12 + 4 + 4]);
	d.extend_from_slice(&spec.format.to_le_bytes());
	d.push(spec.mipmaps);
	d.extend_from_slice(&(-1i32).to_le_bytes());
	d.extend_from_slice(&[0, 0]);
	d.extend_from_slice(&1u16.to_le_bytes());
	if spec.minor >= 3 {
		d.extend_from_slice(&[0; 3]);
		d.extend_from_slice(&1u32.to_le_bytes());
		d.extend_from_slice(&[0; 8]);
		d.extend_from_slice(&[0x30, 0, 0, 0]);
		d.extend_from_slice(&header_size.to_le_bytes());
	}
	d.resize(header_size as usize, 0);
	d.extend_from_slice(body);
	d
}

#[test]
fn rgba_and_bgr() {
	let pixels = [1, 2, 3, 4, 5, 6, 7, 8];
	let (info, texture) = decode(&vtf(&Spec::new(2, 2, 1, 0), &pixels), 1024).unwrap();
	assert_eq!(info.format, Format::Rgba8888);
	assert_eq!((texture.width, texture.height), (2, 1));
	assert_eq!(texture.rgba, pixels);

	let (_, texture) = decode(&vtf(&Spec::new(1, 1, 1, 3), &[10, 20, 30]), 1024).unwrap();
	assert_eq!(texture.rgba, [30, 20, 10, 255]);
}

#[test]
fn picks_the_mipmap_that_fits() {
	// BGR888 8x8 with four mipmaps, smallest first: 1x1, 2x2, 4x4, 8x8.
	let mut spec = Spec::new(2, 8, 8, 3);
	spec.mipmaps = 4;
	let mut body = Vec::new();
	for (side, shade) in [(1, 1u8), (2, 2), (4, 4), (8, 8)] {
		body.extend(std::iter::repeat_n([0, 0, shade], side * side).flatten());
	}
	let file = vtf(&spec, &body);

	let (_, texture) = decode(&file, 1024).unwrap();
	assert_eq!(texture.width, 8);
	assert_eq!(&texture.rgba[..4], [8, 0, 0, 255]);

	let (_, texture) = decode(&file, 3).unwrap();
	assert_eq!((texture.width, texture.height), (2, 2));
	assert_eq!(&texture.rgba[..4], [2, 0, 0, 255]);

	let (_, texture) = decode(&file, 0).unwrap();
	assert_eq!(texture.width, 1);
}

#[test]
fn resources_and_cubemap_faces() {
	let mut spec = Spec::new(5, 1, 1, 2);
	spec.flags = FLAG_ENVMAP;
	spec.mipmaps = 2;
	spec.width = 2;
	spec.height = 2;
	let mut body = Vec::new();
	body.extend(std::iter::repeat_n([9, 9, 9], 6).flatten());
	for face in 0..6u8 {
		body.extend(std::iter::repeat_n([face + 1, 0, 0], 4).flatten());
	}
	let (info, texture) = decode(&vtf(&spec, &body), 1024).unwrap();
	assert_eq!(info.faces, 6);
	assert_eq!(texture.width, 2);
	assert_eq!(&texture.rgba[..4], [1, 0, 0, 255]);
}

#[test]
fn sphere_map_face_only_when_the_file_has_room() {
	let mut spec = Spec::new(2, 1, 1, 5);
	spec.flags = FLAG_ENVMAP;
	assert_eq!(info(&vtf(&spec, &[0; 7])).unwrap().faces, 7);
	assert_eq!(info(&vtf(&spec, &[0; 6])).unwrap().faces, 6);
	spec.start_frame = 0xffff;
	assert_eq!(info(&vtf(&spec, &[0; 7])).unwrap().faces, 6);
}

#[test]
fn dxt1() {
	// Red and blue endpoints, texels using palette entries 0, 1, 2, 3 in turn.
	let mut block = Vec::new();
	block.extend_from_slice(&0xf800u16.to_le_bytes());
	block.extend_from_slice(&0x001fu16.to_le_bytes());
	block.extend_from_slice(&0xe4e4_e4e4u32.to_le_bytes());
	let (_, texture) = decode(&vtf(&Spec::new(2, 4, 4, 13), &block), 1024).unwrap();
	let texel = |i: usize| &texture.rgba[i * 4..i * 4 + 4];
	assert_eq!(texel(0), [255, 0, 0, 255]);
	assert_eq!(texel(1), [0, 0, 255, 255]);
	assert_eq!(texel(2), [170, 0, 85, 255]);
	assert_eq!(texel(3), [85, 0, 170, 255]);

	// Swapped endpoints switch to three colors and transparent black.
	let mut block = Vec::new();
	block.extend_from_slice(&0x001fu16.to_le_bytes());
	block.extend_from_slice(&0xf800u16.to_le_bytes());
	block.extend_from_slice(&3u32.to_le_bytes());
	let (_, texture) = decode(&vtf(&Spec::new(2, 4, 4, 13), &block), 1024).unwrap();
	assert_eq!(&texture.rgba[..8], [0, 0, 0, 0, 0, 0, 255, 255]);
}

#[test]
fn dxt5_alpha_and_partial_blocks() {
	let mut block = vec![255, 0];
	// Alpha index 0 (255) for texel 0, index 1 (0) for texel 1.
	let bits: u64 = 0b001_000;
	block.extend_from_slice(&bits.to_le_bytes()[..6]);
	block.extend_from_slice(&0xffffu16.to_le_bytes());
	block.extend_from_slice(&0xffffu16.to_le_bytes());
	block.extend_from_slice(&0u32.to_le_bytes());
	// A 2x2 texture still takes one whole 4x4 block.
	let (_, texture) = decode(&vtf(&Spec::new(2, 2, 2, 15), &block), 1024).unwrap();
	assert_eq!(texture.rgba.len(), 2 * 2 * 4);
	assert_eq!(&texture.rgba[..8], [255, 255, 255, 255, 255, 255, 255, 0]);
}

#[test]
fn hdr_and_half_floats() {
	assert_eq!(f16_to_f32(0x3c00), 1.0);
	assert_eq!(f16_to_f32(0xc000), -2.0);
	assert_eq!(f16_to_f32(0x0000), 0.0);
	let mut body = Vec::new();
	for half in [0x3c00u16, 0x0000, 0x4000, 0x3800] {
		body.extend_from_slice(&half.to_le_bytes());
	}
	let (_, texture) = decode(&vtf(&Spec::new(2, 1, 1, 24), &body), 1024).unwrap();
	assert_eq!(texture.rgba, [255, 0, 255, 128]);
}

#[test]
fn fully_transparent_shows_opaque() {
	let (_, texture) = decode(
		&vtf(&Spec::new(2, 2, 1, 0), &[1, 2, 3, 0, 4, 5, 6, 0]),
		1024,
	)
	.unwrap();
	assert_eq!(texture.rgba, [1, 2, 3, 255, 4, 5, 6, 255]);
	let (_, texture) = decode(
		&vtf(&Spec::new(2, 2, 1, 0), &[1, 2, 3, 0, 4, 5, 6, 1]),
		1024,
	)
	.unwrap();
	assert_eq!(texture.rgba, [1, 2, 3, 0, 4, 5, 6, 1]);
}

#[test]
fn signed_du_dv() {
	let (_, texture) = decode(&vtf(&Spec::new(2, 1, 1, 22), &[0, 0x80]), 1024).unwrap();
	assert_eq!(texture.rgba, [128, 0, 255, 255]);
}

#[test]
fn errors() {
	assert_eq!(decode(b"PNG\0rest", 1024), Err(Error::NotVtf));
	let mut file = vtf(&Spec::new(2, 1, 1, 0), &[0; 4]);
	file[8] = 9;
	assert_eq!(decode(&file, 1024), Err(Error::Version(7, 9)));
	let file = vtf(&Spec::new(2, 4, 4, 0), &[0; 10]);
	assert_eq!(decode(&file, 1024), Err(Error::Truncated));
	let file = vtf(&Spec::new(2, 4, 4, 7), &[0; 16]);
	assert_eq!(decode(&file, 1024), Err(Error::Format("P8".into())));
	let mut spec = Spec::new(2, 0xffff, 0xffff, 29);
	spec.frames = 0xffff;
	spec.mipmaps = 16;
	assert_eq!(decode(&vtf(&spec, &[]), 65535), Err(Error::Truncated));
}

#[test]
fn png_round_trip() {
	let (_, texture) = decode(&vtf(&Spec::new(2, 1, 1, 0), &[1, 2, 3, 4]), 1024).unwrap();
	let png = texture.to_png();
	let back = image::load_from_memory(&png).unwrap().to_rgba8();
	assert_eq!(back.into_raw(), [1, 2, 3, 4]);
}
