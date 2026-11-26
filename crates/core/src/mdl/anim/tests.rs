use super::*;

/// A version 48 model with bones, one animation and one sequence, written by hand.
pub(crate) struct Builder {
	pub data: Vec<u8>,
	strings: Vec<(usize, usize, String)>,
}

impl Builder {
	pub fn new(size: usize) -> Self {
		let mut data = vec![0; size];
		data[..4].copy_from_slice(b"IDST");
		data[4..8].copy_from_slice(&48i32.to_le_bytes());
		Self {
			data,
			strings: Vec::new(),
		}
	}

	pub fn i32(&mut self, at: usize, v: i32) {
		self.data[at..at + 4].copy_from_slice(&v.to_le_bytes());
	}

	pub fn f32(&mut self, at: usize, v: f32) {
		self.data[at..at + 4].copy_from_slice(&v.to_le_bytes());
	}

	pub fn u16(&mut self, at: usize, v: u16) {
		self.data[at..at + 2].copy_from_slice(&v.to_le_bytes());
	}

	/// A name whose offset at `at` is relative to `base`; written by [`Self::finish`].
	pub fn name(&mut self, base: usize, at: usize, name: &str) {
		self.strings.push((base, at, name.to_owned()));
	}

	pub fn finish(mut self) -> Vec<u8> {
		for (base, at, name) in std::mem::take(&mut self.strings) {
			let start = self.data.len();
			self.data.extend_from_slice(name.as_bytes());
			self.data.push(0);
			self.i32(at, (start - base) as i32);
		}
		let len = self.data.len() as i32;
		self.i32(76, len);
		self.data
	}
}

const BONES: usize = 408;
const DESC: usize = BONES + 2 * BONE_SIZE;
const RECORDS: usize = DESC + ANIM_DESC_SIZE;
const SEQUENCE: usize = 1300;
const MOVEMENT: usize = 1700;

/// Two bones; the root moves along X over three frames, the child is raw data.
pub(crate) fn sample() -> Vec<u8> {
	let mut b = Builder::new(2000);
	b.i32(156, 2);
	b.i32(160, BONES as i32);
	for (i, name) in ["root", "child"].iter().enumerate() {
		let at = BONES + i * BONE_SIZE;
		b.name(at, at, name);
		b.i32(at + 4, i as i32 - 1);
		b.f32(at + 32, 1.0);
		b.f32(at + 36, 2.0);
		b.f32(at + 40, 3.0);
		b.f32(at + 72, 0.5);
	}

	b.i32(180, 1);
	b.i32(184, DESC as i32);
	b.name(DESC, DESC + 4, "@run");
	b.f32(DESC + 8, 30.0);
	b.i32(DESC + 12, STUDIO_LOOPING as i32);
	b.i32(DESC + 16, 3);
	b.i32(DESC + 20, 1);
	b.i32(DESC + 24, (MOVEMENT - DESC) as i32);
	b.i32(DESC + 56, (RECORDS - DESC) as i32);
	b.i32(MOVEMENT, 2);
	b.i32(MOVEMENT + 4, 0x40);
	b.f32(MOVEMENT + 8, 5.0);
	b.f32(MOVEMENT + 12, 5.0);
	b.f32(MOVEMENT + 20, 1.0);
	b.f32(MOVEMENT + 32, 5.0);

	// Root: packed X positions 2 then 4 (scale 0.5), the last repeated; no rotation data.
	b.data[RECORDS] = 0;
	b.data[RECORDS + 1] = ANIM_ANIMPOS;
	let child = RECORDS + 20;
	b.u16(RECORDS + 2, 20);
	b.u16(RECORDS + 4, 6);
	b.data[RECORDS + 10] = 2;
	b.data[RECORDS + 11] = 3;
	b.u16(RECORDS + 12, 2);
	b.u16(RECORDS + 14, 4);
	// Child: a raw identity rotation and a raw position of (1, -2, 0.5).
	b.data[child] = 1;
	b.data[child + 1] = ANIM_RAWROT | ANIM_RAWPOS;
	b.u16(child + 4, 32768);
	b.u16(child + 6, 32768);
	b.u16(child + 8, 16384);
	b.u16(child + 10, 0x3c00);
	b.u16(child + 12, 0xc000);
	b.u16(child + 14, 0x3800);

	b.i32(188, 1);
	b.i32(192, SEQUENCE as i32);
	b.name(SEQUENCE, SEQUENCE + 4, "run");
	b.name(SEQUENCE, SEQUENCE + 8, "ACT_RUN");
	b.i32(SEQUENCE + 12, STUDIO_LOOPING as i32);
	b.i32(SEQUENCE + 20, 1);
	b.i32(SEQUENCE + 24, 1);
	b.i32(SEQUENCE + 28, 220);
	b.f32(SEQUENCE + 220, 0.5);
	b.i32(SEQUENCE + 224, 7);
	b.i32(SEQUENCE + 60, 214);
	b.i32(SEQUENCE + 68, 1);
	b.i32(SEQUENCE + 72, 1);
	b.i32(SEQUENCE + 76, -1);
	b.i32(SEQUENCE + 80, -1);
	b.f32(SEQUENCE + 104, 0.2);
	b.f32(SEQUENCE + 108, 0.3);
	b.finish()
}

fn bind() -> Vec<Pose> {
	vec![
		Pose {
			position: [1.0, 2.0, 3.0],
			rotation: [0.0; 3],
		};
		2
	]
}

#[test]
fn reads_descs_and_sequences() {
	let data = sample();
	let a = parse(&data, 48).unwrap();
	let desc = &a.descs[0];
	assert_eq!(
		(desc.name.as_str(), desc.frames, desc.fps),
		("@run", 3, 30.0)
	);
	assert_eq!(desc.movements[0].end_frame, 2);
	assert_eq!(desc.movements[0].vector, [1.0, 0.0, 0.0]);
	let seq = &a.sequences[0];
	assert_eq!(
		(seq.name.as_str(), seq.activity.as_str()),
		("run", "ACT_RUN")
	);
	assert_eq!(seq.anims, [0]);
	assert_eq!(seq.events[0].cycle, 0.5);
	assert_eq!(seq.events[0].event, 7);
	assert_eq!(seq.fade_out, 0.3);
}

#[test]
fn packed_raw_and_missing_values() {
	let data = sample();
	let a = parse(&data, 48).unwrap();
	let frames = a.poses(&a.descs[0], &data, None, &bind()).unwrap();
	let root_x: Vec<f32> = frames.iter().map(|f| f[0].position[0]).collect();
	// Packed values are added to the bind pose.
	assert_eq!(root_x, [2.0, 3.0, 3.0]);
	assert_eq!(frames[0][0].position[1..], [2.0, 3.0]);
	assert_eq!(frames[0][0].rotation, [0.0; 3]);
	assert_eq!(frames[2][1].position, [1.0, -2.0, 0.5]);
	assert!(frames[2][1].rotation.iter().all(|v| v.abs() < 1e-6));
}

#[test]
fn a_missing_ani_is_an_error() {
	let mut data = sample();
	// Point the animation at block 1 of an .ani.
	data[DESC + 52..DESC + 56].copy_from_slice(&1i32.to_le_bytes());
	let mut a = parse(&data, 48).unwrap();
	a.blocks = vec![(0, 0), (0, 100)];
	assert_eq!(
		a.poses(&a.descs[0], &data, None, &bind()),
		Err(Error::MissingAni)
	);
}

#[test]
fn run_length_values() {
	// Runs: [valid 2, total 3: 10 20], [valid 1, total 2: 30], end.
	let mut data = Vec::new();
	for v in [2u8, 3] {
		data.push(v);
	}
	for v in [10i16, 20] {
		data.extend_from_slice(&v.to_le_bytes());
	}
	data.extend_from_slice(&[1, 2]);
	data.extend_from_slice(&30i16.to_le_bytes());
	data.extend_from_slice(&[0, 0]);
	let r = Reader(&data);
	let values: Vec<i16> = (0..6).map(|f| value_at(r, 0, f).unwrap()).collect();
	assert_eq!(values, [10, 20, 20, 30, 30, 0]);
}

#[test]
fn packed_numbers() {
	assert_eq!(half(0x3c00), 1.0);
	assert_eq!(half(0xc000), -2.0);
	assert_eq!(half(0x0001), 2f32.powi(-24));
	let [x, y, z, w] = quaternion48([0, 0x80, 0, 0x80, 0, 0xc0]);
	assert_eq!([x, y, z], [0.0, 0.0, 0.0]);
	assert_eq!(w, -1.0);
	let [x, y, z, w] =
		quaternion64((1_048_576u64 | 1_048_576 << 21 | 1_048_576 << 42).to_le_bytes());
	assert!([x, y, z].iter().all(|v| v.abs() < 1e-6));
	assert!((w - 1.0).abs() < 1e-6);
}

#[test]
fn quaternion_to_roll_pitch_yaw() {
	let half_turn = std::f32::consts::FRAC_1_SQRT_2;
	// 90° about Z is a yaw.
	let [roll, pitch, yaw] = quaternion_angles([0.0, 0.0, half_turn, half_turn]);
	assert!(roll.abs() < 1e-6 && pitch.abs() < 1e-6);
	assert!((yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
	// 90° about X is a roll.
	let [roll, _, _] = quaternion_angles([half_turn, 0.0, 0.0, half_turn]);
	assert!((roll - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
}
