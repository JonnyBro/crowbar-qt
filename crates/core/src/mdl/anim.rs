//! Animations, sequences and what they refer to: pose parameters, IK chains, nodes.

use super::{Error, Reader, Result};

pub const STUDIO_LOOPING: u32 = 0x1;
pub const STUDIO_SNAP: u32 = 0x2;
pub const STUDIO_DELTA: u32 = 0x4;
pub const STUDIO_AUTOPLAY: u32 = 0x8;
pub const STUDIO_POST: u32 = 0x10;
pub const STUDIO_ALLZEROS: u32 = 0x20;
pub const STUDIO_FRAMEANIM: u32 = 0x40;
pub const STUDIO_CYCLEPOSE: u32 = 0x80;
pub const STUDIO_REALTIME: u32 = 0x100;
pub const STUDIO_LOCAL: u32 = 0x200;
pub const STUDIO_HIDDEN: u32 = 0x400;
pub const STUDIO_OVERRIDE: u32 = 0x800;
pub const STUDIO_WORLD: u32 = 0x4000;

const ANIM_RAWPOS: u8 = 0x01;
const ANIM_RAWROT: u8 = 0x02;
const ANIM_ANIMPOS: u8 = 0x04;
const ANIM_ANIMROT: u8 = 0x08;
const ANIM_DELTA: u8 = 0x10;
const ANIM_RAWROT2: u8 = 0x20;

const BONE_SIZE: usize = 216;
const ANIM_DESC_SIZE: usize = 100;
const SEQUENCE_SIZE: usize = 212;
const MOVEMENT_SIZE: usize = 44;
const EVENT_SIZE: usize = 80;
const LAYER_SIZE: usize = 24;
const IK_LOCK_SIZE: usize = 32;
const IK_CHAIN_SIZE: usize = 16;
const IK_LINK_SIZE: usize = 28;
const POSE_PARAMETER_SIZE: usize = 20;
const IK_RULE_SIZE: usize = 152;
/// Events with a name instead of a number.
const NEW_EVENT_STYLE: i32 = 1 << 10;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Animations {
	pub descs: Vec<AnimDesc>,
	pub sequences: Vec<SequenceDesc>,
	pub pose_parameters: Vec<PoseParameter>,
	pub ik_chains: Vec<IkChain>,
	pub ik_autoplay_locks: Vec<IkLock>,
	pub nodes: Vec<String>,
	/// Start and end of each block in the .ani; block 0 is unused.
	pub blocks: Vec<(usize, usize)>,
	/// Per bone: position and rotation scale of the packed values.
	scales: Vec<([f32; 3], [f32; 3])>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimDesc {
	pub name: String,
	pub fps: f32,
	pub flags: u32,
	pub frames: usize,
	pub movements: Vec<Movement>,
	pub section_frames: usize,
	pub ik_rules: usize,
	/// Where the desc is; data offsets are relative to it.
	at: usize,
	block: i32,
	index: i32,
	ik_rule_index: i32,
	ik_rule_block_index: i32,
	/// Block and offset of each section.
	sections: Vec<(i32, i32)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Movement {
	pub end_frame: i32,
	pub flags: u32,
	pub v0: f32,
	pub v1: f32,
	/// Degrees of yaw.
	pub angle: f32,
	pub vector: [f32; 3],
	pub position: [f32; 3],
}

pub const IK_SELF: i32 = 1;
pub const IK_WORLD: i32 = 2;
pub const IK_GROUND: i32 = 3;
pub const IK_RELEASE: i32 = 4;
pub const IK_ATTACHMENT: i32 = 5;
pub const IK_UNLATCH: i32 = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct IkRule {
	pub kind: i32,
	pub chain: i32,
	pub bone: i32,
	pub slot: i32,
	pub height: f32,
	pub radius: f32,
	pub floor: f32,
	pub position: [f32; 3],
	pub rotation: [f32; 4],
	/// Cycles, 0 to 1.
	pub start: f32,
	pub peak: f32,
	pub tail: f32,
	pub end: f32,
	pub contact: f32,
	pub attachment: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceDesc {
	pub name: String,
	pub activity: String,
	pub activity_weight: i32,
	pub flags: u32,
	pub events: Vec<Event>,
	/// Animation desc indexes, `group_size[0]` per row.
	pub anims: Vec<usize>,
	pub group_size: [usize; 2],
	pub param_index: [i32; 2],
	pub param_start: [f32; 2],
	pub param_end: [f32; 2],
	pub fade_in: f32,
	pub fade_out: f32,
	/// 1-based into `nodes`; 0 for none.
	pub entry_node: i32,
	pub exit_node: i32,
	pub node_flags: i32,
	pub layers: Vec<Layer>,
	/// Per bone; empty when the sequence has none.
	pub weights: Vec<f32>,
	pub ik_locks: Vec<IkLock>,
	pub key_values: String,
	pub cycle_pose: i32,
	pub activity_modifiers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
	pub cycle: f32,
	pub event: i32,
	pub name: String,
	pub options: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
	pub sequence: i16,
	pub pose: i16,
	pub flags: u32,
	pub start: f32,
	pub peak: f32,
	pub tail: f32,
	pub end: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IkLock {
	pub chain: i32,
	pub position_weight: f32,
	pub local_rotation_weight: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IkChain {
	pub name: String,
	pub bones: Vec<i32>,
	pub knee: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoseParameter {
	pub name: String,
	pub flags: u32,
	pub start: f32,
	pub end: f32,
	pub looping: f32,
}

/// A bone relative to its parent; rotation as roll, pitch, yaw in radians.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pose {
	pub position: [f32; 3],
	pub rotation: [f32; 3],
}

pub fn parse(data: &[u8], version: i32) -> Result<Animations> {
	let r = Reader(data);
	let vec3 = |at: usize| -> Result<[f32; 3]> { Ok([r.f32(at)?, r.f32(at + 4)?, r.f32(at + 8)?]) };
	let scales = r.table(156, BONE_SIZE, |at| Ok((vec3(at + 72)?, vec3(at + 84)?)))?;
	let descs = r.table(180, ANIM_DESC_SIZE, |at| {
		let frames = r.count(at + 16)?;
		let section_frames = r.count(at + 84)?;
		let sections = match (section_frames, r.offset(at + 80)?) {
			(0, _) | (_, 0) => Vec::new(),
			(n, offset) => (0..frames / n + 2)
				.map(|i| {
					let s = at + offset + i * 8;
					Ok((r.i32(s)?, r.i32(s + 4)?))
				})
				.collect::<Result<_>>()?,
		};
		let movements = table_at(r, at, at + 20, MOVEMENT_SIZE, |m| {
			Ok(Movement {
				end_frame: r.i32(m)?,
				flags: r.i32(m + 4)? as u32,
				v0: r.f32(m + 8)?,
				v1: r.f32(m + 12)?,
				angle: r.f32(m + 16)?,
				vector: vec3(m + 20)?,
				position: vec3(m + 32)?,
			})
		})?;
		Ok(AnimDesc {
			name: r.name_at(at, at + 4)?,
			fps: r.f32(at + 8)?,
			flags: r.i32(at + 12)? as u32,
			frames,
			movements,
			section_frames,
			ik_rules: r.count(at + 60)?,
			at,
			block: r.i32(at + 52)?,
			index: r.i32(at + 56)?,
			ik_rule_index: r.i32(at + 64)?,
			ik_rule_block_index: r.i32(at + 68)?,
			sections,
		})
	})?;
	let sequences = r.table(188, SEQUENCE_SIZE, |at| {
		sequence(r, at, scales.len(), version)
	})?;
	let pose_parameters = r.table(300, POSE_PARAMETER_SIZE, |at| {
		Ok(PoseParameter {
			name: r.name_at(at, at)?,
			flags: r.i32(at + 4)? as u32,
			start: r.f32(at + 8)?,
			end: r.f32(at + 12)?,
			looping: r.f32(at + 16)?,
		})
	})?;
	let ik_chains = r.table(284, IK_CHAIN_SIZE, |at| {
		let links = table_at(r, at, at + 8, IK_LINK_SIZE, |l| {
			Ok((r.i32(l)?, vec3(l + 4)?))
		})?;
		Ok(IkChain {
			name: r.name_at(at, at)?,
			bones: links.iter().map(|l| l.0).collect(),
			knee: links.first().map_or([0.0; 3], |l| l.1),
		})
	})?;
	let ik_autoplay_locks = r.table(320, IK_LOCK_SIZE, |at| ik_lock(r, at))?;
	let node_count = r.count(248)?;
	let names = r.offset(256)?;
	let nodes = (0..node_count)
		.map(|i| r.string(r.offset(names + i * 4)?))
		.collect::<Result<_>>()?;
	// Offsets into the .ani, which can be bigger than the .mdl.
	let blocks = r.table(352, 8, |at| {
		Ok((r.i32(at)?.max(0) as usize, r.i32(at + 4)?.max(0) as usize))
	})?;
	Ok(Animations {
		descs,
		sequences,
		pose_parameters,
		ik_chains,
		ik_autoplay_locks,
		nodes,
		blocks,
		scales,
	})
}

fn sequence(r: Reader, at: usize, bones: usize, version: i32) -> Result<SequenceDesc> {
	let group_size = [r.count(at + 68)?, r.count(at + 72)?];
	let anim_count = group_size[0].saturating_mul(group_size[1]);
	let anims_at = at + r.offset(at + 60)?;
	let anims = (0..anim_count)
		.map(|i| Ok(r.u16(anims_at + i * 2)? as usize))
		.collect::<Result<_>>()?;
	let events = table_at(r, at, at + 24, EVENT_SIZE, |e| {
		let kind = r.i32(e + 8)?;
		Ok(Event {
			cycle: r.f32(e)?,
			event: r.i32(e + 4)?,
			name: if kind & NEW_EVENT_STYLE != 0 {
				r.name_at(e, e + 76)?
			} else {
				String::new()
			},
			options: r.fixed_string(e + 12, 64)?,
		})
	})?;
	let layers = table_at(r, at, at + 148, LAYER_SIZE, |l| {
		Ok(Layer {
			sequence: r.u16(l)? as i16,
			pose: r.u16(l + 2)? as i16,
			flags: r.i32(l + 4)? as u32,
			start: r.f32(l + 8)?,
			peak: r.f32(l + 12)?,
			tail: r.f32(l + 16)?,
			end: r.f32(l + 20)?,
		})
	})?;
	let weights = match r.offset(at + 156)? {
		0 => Vec::new(),
		offset => (0..bones)
			.map(|b| r.f32(at + offset + b * 4))
			.collect::<Result<_>>()?,
	};
	let ik_locks = table_at(r, at, at + 164, IK_LOCK_SIZE, |l| ik_lock(r, l))?;
	let key_values = match (r.offset(at + 172)?, r.count(at + 176)?) {
		(_, 0) | (0, _) => String::new(),
		(offset, size) => {
			let bytes =
				r.0.get(at + offset..at + offset + size)
					.ok_or(Error::Truncated)?;
			let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
			String::from_utf8_lossy(&bytes[..end]).into_owned()
		}
	};
	let activity_modifiers = if version >= 49 {
		let count = r.count(at + 188)?;
		let start = at + r.offset(at + 184)?;
		(0..count)
			.map(|i| r.name_at(start + i * 4, start + i * 4))
			.collect::<Result<_>>()?
	} else {
		Vec::new()
	};
	Ok(SequenceDesc {
		name: r.name_at(at, at + 4)?,
		activity: r.name_at(at, at + 8)?,
		activity_weight: r.i32(at + 20)?,
		flags: r.i32(at + 12)? as u32,
		events,
		anims,
		group_size,
		param_index: [r.i32(at + 76)?, r.i32(at + 80)?],
		param_start: [r.f32(at + 84)?, r.f32(at + 88)?],
		param_end: [r.f32(at + 92)?, r.f32(at + 96)?],
		fade_in: r.f32(at + 104)?,
		fade_out: r.f32(at + 108)?,
		entry_node: r.i32(at + 112)?,
		exit_node: r.i32(at + 116)?,
		node_flags: r.i32(at + 120)?,
		layers,
		weights,
		ik_locks,
		key_values,
		cycle_pose: r.i32(at + 180)?,
		activity_modifiers,
	})
}

fn ik_lock(r: Reader, at: usize) -> Result<IkLock> {
	Ok(IkLock {
		chain: r.i32(at)?,
		position_weight: r.f32(at + 4)?,
		local_rotation_weight: r.f32(at + 8)?,
	})
}

/// A count at `at` and an offset after it, relative to `base`.
pub(super) fn table_at<T>(
	r: Reader,
	base: usize,
	at: usize,
	size: usize,
	mut read: impl FnMut(usize) -> Result<T>,
) -> Result<Vec<T>> {
	let count = r.count(at)?;
	if count == 0 {
		return Ok(Vec::new());
	}
	let start = base + r.offset(at + 4)?;
	if start.saturating_add(count.saturating_mul(size)) > r.0.len() {
		return Err(Error::Truncated);
	}
	(0..count).map(|i| read(start + i * size)).collect()
}

impl Animations {
	/// Every frame of `desc`, each bone relative to its parent as stored: the
	/// root turned by studiomdl and without the movement it took out.
	pub fn poses(
		&self,
		desc: &AnimDesc,
		mdl: &[u8],
		ani: Option<&[u8]>,
		bind: &[Pose],
	) -> Result<Vec<Vec<Pose>>> {
		let delta = desc.flags & STUDIO_DELTA != 0;
		let base: Vec<Pose> = if delta {
			vec![Pose::default(); bind.len()]
		} else {
			bind.to_vec()
		};
		let mut frames = Vec::with_capacity(desc.frames);
		for frame in 0..desc.frames.max(1) {
			let mut pose = base.clone();
			if desc.flags & STUDIO_ALLZEROS == 0
				&& let Some((data, at, local)) = self.locate(desc, frame, mdl, ani)?
			{
				self.read_frame(Reader(data), at, local, bind, &mut pose)?;
			}
			frames.push(pose);
		}
		Ok(frames)
	}

	/// The rules are in the .mdl, or in the .ani block the animation is in.
	pub fn ik_rules(&self, desc: &AnimDesc, mdl: &[u8], ani: Option<&[u8]>) -> Result<Vec<IkRule>> {
		if desc.ik_rules == 0 {
			return Ok(Vec::new());
		}
		let (data, start) = if desc.ik_rule_index != 0 {
			(mdl, desc.at as i64 + desc.ik_rule_index as i64)
		} else if desc.block > 0 {
			// Often right at the start of the block, at offset 0.
			let (block, _) = *self
				.blocks
				.get(desc.block as usize)
				.ok_or(Error::Truncated)?;
			(
				ani.ok_or(Error::MissingAni)?,
				block as i64 + desc.ik_rule_block_index as i64,
			)
		} else if desc.ik_rule_block_index != 0 {
			(mdl, desc.at as i64 + desc.ik_rule_block_index as i64)
		} else {
			return Ok(Vec::new());
		};
		let r = Reader(data);
		let start = usize::try_from(start).map_err(|_| Error::Truncated)?;
		(0..desc.ik_rules)
			.map(|i| {
				let at = start + i * IK_RULE_SIZE;
				let f = |o: usize| r.f32(at + o);
				Ok(IkRule {
					kind: r.i32(at + 4)?,
					chain: r.i32(at + 8)?,
					bone: r.i32(at + 12)?,
					slot: r.i32(at + 16)?,
					height: f(20)?,
					radius: f(24)?,
					floor: f(28)?,
					position: [f(32)?, f(36)?, f(40)?],
					rotation: [f(44)?, f(48)?, f(52)?, f(56)?],
					start: f(76)?,
					peak: f(80)?,
					tail: f(84)?,
					end: f(88)?,
					contact: f(96)?,
					attachment: r.name_at(at, at + 120)?,
				})
			})
			.collect()
	}

	/// The data the frame is in, where its first bone record is, and the frame within it.
	fn locate<'a>(
		&self,
		desc: &AnimDesc,
		frame: usize,
		mdl: &'a [u8],
		ani: Option<&'a [u8]>,
	) -> Result<Option<(&'a [u8], usize, usize)>> {
		let (mut block, mut index, mut local) = (desc.block, desc.index, frame);
		if desc.section_frames != 0 && !desc.sections.is_empty() {
			// The last frame of a long animation is stored on its own.
			let section = if desc.frames > desc.section_frames && frame == desc.frames - 1 {
				local = 0;
				desc.frames / desc.section_frames + 1
			} else {
				let s = frame / desc.section_frames;
				local = frame - s * desc.section_frames;
				s
			};
			(block, index) = *desc.sections.get(section).ok_or(Error::Truncated)?;
		}
		match block {
			-1 => Ok(None),
			0 => {
				let at = desc.at as i64 + index as i64;
				Ok(Some((
					mdl,
					usize::try_from(at).map_err(|_| Error::Truncated)?,
					local,
				)))
			}
			b => {
				let (start, _) = *self.blocks.get(b as usize).ok_or(Error::Truncated)?;
				let ani = ani.ok_or(Error::MissingAni)?;
				let at = start as i64 + index as i64;
				Ok(Some((
					ani,
					usize::try_from(at).map_err(|_| Error::Truncated)?,
					local,
				)))
			}
		}
	}

	fn read_frame(
		&self,
		r: Reader,
		mut at: usize,
		frame: usize,
		bind: &[Pose],
		pose: &mut [Pose],
	) -> Result<()> {
		for _ in 0..=bind.len() {
			let [bone, flags] = r.bytes::<2>(at)?;
			if bone == 255 {
				break;
			}
			let next = r.u16(at + 2)? as i16;
			let bone = bone as usize;
			let (Some(out), Some(rest), Some(&(position_scale, rotation_scale))) =
				(pose.get_mut(bone), bind.get(bone), self.scales.get(bone))
			else {
				return Err(Error::Truncated);
			};
			let data = at + 4;
			let delta = flags & ANIM_DELTA != 0;
			if flags & ANIM_RAWROT != 0 {
				out.rotation = quaternion_angles(quaternion48(r.bytes(data)?));
			} else if flags & ANIM_RAWROT2 != 0 {
				out.rotation = quaternion_angles(quaternion64(r.bytes(data)?));
			} else if flags & ANIM_ANIMROT != 0 {
				let mut angles = values(r, data, frame, rotation_scale)?;
				if !delta {
					angles = [0, 1, 2].map(|i| angles[i] + rest.rotation[i]);
				}
				out.rotation = angles;
			} else if delta {
				out.rotation = [0.0; 3];
			} else {
				out.rotation = rest.rotation;
			}
			let rotation_size = if flags & ANIM_RAWROT != 0 {
				6
			} else if flags & ANIM_RAWROT2 != 0 {
				8
			} else if flags & ANIM_ANIMROT != 0 {
				6
			} else {
				0
			};
			if flags & ANIM_RAWPOS != 0 {
				let raw: [u8; 6] = r.bytes(data + rotation_size)?;
				out.position =
					[0, 1, 2].map(|i| half(u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]])));
			} else if flags & ANIM_ANIMPOS != 0 {
				let mut position = values(r, data + rotation_size, frame, position_scale)?;
				if !delta {
					position = [0, 1, 2].map(|i| position[i] + rest.position[i]);
				}
				out.position = position;
			} else if delta {
				out.position = [0.0; 3];
			} else {
				out.position = rest.position;
			}
			if next == 0 {
				break;
			}
			at = usize::try_from(at as i64 + next as i64).map_err(|_| Error::Truncated)?;
		}
		Ok(())
	}
}

/// Three channels behind offsets relative to `at`; offset 0 is a zero channel.
fn values(r: Reader, at: usize, frame: usize, scale: [f32; 3]) -> Result<[f32; 3]> {
	let mut out = [0.0; 3];
	for (i, value) in out.iter_mut().enumerate() {
		let offset = r.u16(at + i * 2)? as i16;
		if offset != 0 {
			let start = usize::try_from(at as i64 + offset as i64).map_err(|_| Error::Truncated)?;
			*value = value_at(r, start, frame)? as f32 * scale[i];
		}
	}
	Ok(out)
}

/// Run-length packed values: each run is `valid` stored values covering `total` frames,
/// the last one repeated.
fn value_at(r: Reader, mut at: usize, frame: usize) -> Result<i16> {
	let mut k = frame;
	loop {
		let [valid, total] = r.bytes::<2>(at)?;
		if total == 0 {
			return Ok(0);
		}
		if (total as usize) > k {
			let i = if (valid as usize) > k {
				k + 1
			} else {
				valid as usize
			};
			return Ok(r.u16(at + i * 2)? as i16);
		}
		k -= total as usize;
		at += (valid as usize + 1) * 2;
	}
}

fn quaternion48(b: [u8; 6]) -> [f32; 4] {
	let x = u16::from_le_bytes([b[0], b[1]]);
	let y = u16::from_le_bytes([b[2], b[3]]);
	let zw = u16::from_le_bytes([b[4], b[5]]);
	let x = (x as f32 - 32768.0) / 32768.0;
	let y = (y as f32 - 32768.0) / 32768.0;
	let z = ((zw & 0x7fff) as f32 - 16384.0) / 16384.0;
	let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
	[x, y, z, if zw & 0x8000 != 0 { -w } else { w }]
}

fn quaternion64(b: [u8; 8]) -> [f32; 4] {
	let bits = u64::from_le_bytes(b);
	let part = |shift: u32| ((bits >> shift) & 0x1f_ffff) as f64;
	let [x, y, z] = [0, 21, 42].map(|s| ((part(s) - 1_048_576.0) * (1.0 / 1_048_576.5)) as f32);
	let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
	[x, y, z, if bits >> 63 != 0 { -w } else { w }]
}

pub(super) fn half(h: u16) -> f32 {
	let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
	let exponent = ((h >> 10) & 0x1f) as i32;
	let mantissa = (h & 0x3ff) as f32;
	sign * match exponent {
		0 => mantissa / 1024.0 * 2f32.powi(-14),
		31 => {
			if mantissa == 0.0 {
				f32::INFINITY
			} else {
				f32::NAN
			}
		}
		e => (1.0 + mantissa / 1024.0) * 2f32.powi(e - 15),
	}
}

/// Roll, pitch and yaw in radians, as the engine's QuaternionAngles gives them.
pub fn quaternion_angles([x, y, z, w]: [f32; 4]) -> [f32; 3] {
	let forward = [
		1.0 - 2.0 * y * y - 2.0 * z * z,
		2.0 * x * y + 2.0 * w * z,
		2.0 * x * z - 2.0 * w * y,
	];
	let left = [
		2.0 * x * y - 2.0 * w * z,
		1.0 - 2.0 * x * x - 2.0 * z * z,
		2.0 * y * z + 2.0 * w * x,
	];
	let up_z = 1.0 - 2.0 * x * x - 2.0 * y * y;
	let xy = (forward[0] * forward[0] + forward[1] * forward[1]).sqrt();
	let pitch = (-forward[2]).atan2(xy);
	if xy > 0.001 {
		[left[2].atan2(up_z), pitch, forward[1].atan2(forward[0])]
	} else {
		[0.0, pitch, (-left[0]).atan2(left[1])]
	}
}

#[cfg(test)]
pub(crate) mod tests;
