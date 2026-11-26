//! Animation SMDs and the `$animation`, `$sequence` and related QC commands.

use std::collections::HashSet;
use std::fmt::Write;

use super::{File, num, quote, smd_name, unique, vec3};
use crate::mdl::Model;
use crate::mdl::anim::{self, AnimDesc, Animations, IkRule, Movement, Pose, SequenceDesc};

const LAYER_SPLINE: u32 = 0x40;
const LAYER_XFADE: u32 = 0x80;
const LAYER_NOBLEND: u32 = 0x200;
const LAYER_LOCAL: u32 = 0x1000;
const LAYER_POSE: u32 = 0x4000;
const DEFAULT_FADE: f32 = 0.2;
const MOTION_FLAGS: [(u32, &str); 6] = [
	(0x40, "LX"),
	(0x80, "LY"),
	(0x100, "LZ"),
	(0x200, "LXR"),
	(0x400, "LYR"),
	(0x800, "LZR"),
];

pub(super) fn animations(
	model: &Model,
	anims: &Animations,
	mdl: &[u8],
	ani: Option<&[u8]>,
	stem: &str,
	used: &mut HashSet<String>,
	files: &mut Vec<File>,
) -> Result<String, String> {
	let bind: Vec<Pose> = model
		.bones
		.iter()
		.map(|b| Pose {
			position: b.position,
			rotation: b.rotation,
		})
		.collect();
	let mut paths = Vec::with_capacity(anims.descs.len());
	let mut rules = Vec::with_capacity(anims.descs.len());
	for desc in &anims.descs {
		rules.push(
			anims
				.ik_rules(desc, mdl, ani)
				.map_err(|e| format!("IK rules of \"{}\": {e}", desc.name))?,
		);
		if desc.flags & anim::STUDIO_OVERRIDE != 0 {
			paths.push(String::new());
			continue;
		}
		let frames = anims
			.poses(desc, mdl, ani, &bind)
			.map_err(|e| format!("animation \"{}\": {e}", desc.name))?;
		let name = smd_name(desc.name.trim_start_matches('@'), "anim");
		let path = unique(used, &format!("{stem}_anims/{name}"));
		files.push(File {
			path: path.clone(),
			text: smd(model, &desc.movements, &frames),
		});
		paths.push(path);
	}

	// studiomdl makes a delta only by subtracting: from a pose of zeros the values stay as stored.
	let mut zero = None;
	if anims
		.descs
		.iter()
		.any(|d| d.flags & anim::STUDIO_DELTA != 0)
	{
		let mut name = "delta_zero".to_owned();
		while anims
			.descs
			.iter()
			.any(|d| d.name.eq_ignore_ascii_case(&name))
		{
			name.push('_');
		}
		let path = unique(used, &format!("{stem}_anims/{name}"));
		let pose = vec![Pose::default(); model.bones.len()];
		files.push(File {
			path: path.clone(),
			text: smd(model, &[], std::slice::from_ref(&pose)),
		});
		zero = Some((name, path));
	}
	let subtract = zero.as_ref().map(|(name, _)| name.as_str());

	let mut q = String::new();
	if model.anim_blocks > 0 {
		q.push_str("$animblocksize 32\n");
	}
	let sectioned = anims.descs.iter().filter(|d| d.section_frames > 0);
	if let Some(frames) = sectioned.clone().map(|d| d.section_frames).next() {
		// Animations at least this long get sections.
		let shortest = sectioned.map(|d| d.frames).min().unwrap_or(0);
		let _ = writeln!(q, "$sectionframes {frames} {shortest}");
	}
	for p in &anims.pose_parameters {
		let _ = write!(
			q,
			"$poseparameter {} {} {}",
			quote(&p.name),
			num(p.start),
			num(p.end)
		);
		if p.looping != 0.0 {
			let _ = write!(q, " loop {}", num(p.looping));
		}
		q.push('\n');
	}
	let bone = |i: i32| {
		usize::try_from(i)
			.ok()
			.and_then(|i| model.bones.get(i))
			.map_or("", |b| b.name.as_str())
	};
	let chain = |i: i32| {
		usize::try_from(i)
			.ok()
			.and_then(|i| anims.ik_chains.get(i))
			.map_or("", |c| c.name.as_str())
	};
	for c in &anims.ik_chains {
		let foot = c.bones.last().copied().unwrap_or(-1);
		let _ = writeln!(
			q,
			"$ikchain {} {} knee {}",
			quote(&c.name),
			quote(bone(foot)),
			vec3(c.knee)
		);
	}
	for l in &anims.ik_autoplay_locks {
		let _ = writeln!(
			q,
			"$ikautoplaylock {} {} {}",
			quote(chain(l.chain)),
			num(l.position_weight),
			num(l.local_rotation_weight)
		);
	}
	let weight_lists = weight_lists(&anims.sequences);
	for (name, weights) in &weight_lists {
		let _ = writeln!(q, "$weightlist {} {{", quote(name));
		for (b, w) in model.bones.iter().zip(weights) {
			let _ = writeln!(q, "\t{} {}", quote(&b.name), num(*w));
		}
		q.push_str("}\n");
	}
	if let Some((name, path)) = &zero {
		let noautoik = if anims.ik_chains.is_empty() {
			""
		} else {
			"\tnoautoik\n"
		};
		let _ = writeln!(
			q,
			"$animation {} {} {{\n{noautoik}}}",
			quote(name),
			quote(path)
		);
	}
	if !q.is_empty() {
		q.push('\n');
	}

	let chain_names: Vec<&str> = anims.ik_chains.iter().map(|c| c.name.as_str()).collect();
	for ((desc, path), rules) in anims.descs.iter().zip(&paths).zip(&rules) {
		if desc.name.starts_with('@') {
			continue;
		}
		if path.is_empty() {
			let _ = writeln!(q, "$declareanimation {}", quote(&desc.name));
			continue;
		}
		let _ = writeln!(q, "$animation {} {} {{", quote(&desc.name), quote(path));
		anim_options(
			&mut q,
			desc,
			desc.flags & anim::STUDIO_LOOPING != 0,
			subtract,
		);
		ik_rules(&mut q, model, desc, rules, &chain_names);
		q.push_str("}\n");
	}
	if anims.descs.iter().any(|d| !d.name.starts_with('@')) {
		q.push('\n');
	}

	for seq in &anims.sequences {
		if seq.flags & anim::STUDIO_OVERRIDE != 0 {
			let _ = writeln!(q, "$declaresequence {}", quote(&seq.name));
			continue;
		}
		let used_anims: Vec<&AnimDesc> = seq
			.anims
			.iter()
			.filter_map(|&a| anims.descs.get(a))
			.collect();
		if used_anims.is_empty() {
			continue;
		}
		let _ = writeln!(q, "$sequence {} {{", quote(&seq.name));
		for &a in &seq.anims {
			let Some(desc) = anims.descs.get(a) else {
				continue;
			};
			let name = if desc.name.starts_with('@') {
				paths[a].as_str()
			} else {
				desc.name.as_str()
			};
			let _ = writeln!(q, "\t{}", quote(name));
		}
		let implied = seq
			.anims
			.iter()
			.copied()
			.find(|&a| anims.descs.get(a).is_some_and(|d| d.name.starts_with('@')));
		if let Some(a) = implied {
			anim_options(&mut q, &anims.descs[a], false, subtract);
			ik_rules(&mut q, model, &anims.descs[a], &rules[a], &chain_names);
		}
		sequence_options(&mut q, anims, seq, used_anims[0], &weight_lists);
		q.push_str("}\n");
	}
	Ok(q)
}

fn anim_options(q: &mut String, desc: &AnimDesc, looping: bool, zero: Option<&str>) {
	let _ = writeln!(q, "\tfps {}", num(desc.fps));
	if looping {
		q.push_str("\tloop\n");
	}
	if desc.flags & anim::STUDIO_DELTA != 0
		&& let Some(zero) = zero
	{
		let _ = writeln!(q, "\tsubtract {} 0", quote(zero));
	}
	for m in &desc.movements {
		let flags: Vec<&str> = MOTION_FLAGS
			.iter()
			.filter(|(bit, _)| m.flags & bit != 0)
			.map(|(_, name)| *name)
			.collect();
		if !flags.is_empty() {
			let _ = writeln!(q, "\twalkframe {} {}", m.end_frame, flags.join(" "));
		}
	}
}

fn ik_rules(q: &mut String, model: &Model, desc: &AnimDesc, rules: &[IkRule], chains: &[&str]) {
	// Otherwise studiomdl adds a release rule for each chain the animation leaves out.
	if !chains.is_empty() {
		q.push_str("\tnoautoik\n");
	}
	let last_frame = desc.frames.saturating_sub(1) as f32;
	let frame = |cycle: f32| num(frame_value(cycle, last_frame));
	for rule in rules {
		let chain = usize::try_from(rule.chain)
			.ok()
			.and_then(|i| chains.get(i))
			.copied()
			.unwrap_or("");
		let kind = match rule.kind {
			anim::IK_SELF => {
				let bone = usize::try_from(rule.bone)
					.ok()
					.and_then(|b| model.bones.get(b))
					.map_or("", |b| b.name.as_str());
				format!("touch {}", quote(bone))
			}
			anim::IK_GROUND => "footstep".into(),
			anim::IK_RELEASE => "release".into(),
			anim::IK_ATTACHMENT => format!("attachment {}", quote(&rule.attachment)),
			anim::IK_UNLATCH => "unlatch".into(),
			other => {
				let _ = writeln!(
					q,
					"\t// ikrule {} of type {other} has no QC form",
					quote(chain)
				);
				continue;
			}
		};
		let mut range =
			[rule.start, rule.peak, rule.tail, rule.end].map(|c| frame_value(c, last_frame));
		// Only a looping animation may wrap round; studiomdl refuses it in the others.
		if desc.flags & anim::STUDIO_LOOPING == 0 {
			for i in 1..4 {
				range[i] = range[i].max(range[i - 1]);
			}
		}
		let _ = write!(
			q,
			"\tikrule {} {kind} height {} radius {} floor {} target {} range {} {} {} {} contact {}",
			quote(chain),
			num(rule.height),
			num(rule.radius),
			num(rule.floor),
			rule.slot,
			num(range[0]),
			num(range[1]),
			num(range[2]),
			num(range[3]),
			frame(rule.contact),
		);
		// studiomdl works the rest out itself for the other kinds.
		if matches!(rule.kind, anim::IK_SELF | anim::IK_ATTACHMENT) {
			let [roll, pitch, yaw] = anim::quaternion_angles(rule.rotation).map(f32::to_degrees);
			let _ = write!(
				q,
				" fakeorigin {} fakerotate {}",
				vec3(rule.position),
				vec3([pitch, yaw, roll])
			);
		}
		q.push('\n');
	}
}

/// Cycles can be a little past the end when studiomdl dropped a frame; well past it they wrap.
fn frame_value(cycle: f32, last_frame: f32) -> f32 {
	let mut f = (cycle * last_frame * 1000.0).round() / 1000.0;
	while last_frame > 0.0 && f > last_frame + 1.0 {
		f -= last_frame;
	}
	f.clamp(0.0, last_frame)
}

fn delta(q: &mut String, flags: u32) {
	if flags & anim::STUDIO_DELTA != 0 {
		q.push_str(if flags & anim::STUDIO_POST != 0 {
			"\tdelta\n"
		} else {
			"\tpredelta\n"
		});
	}
}

fn sequence_options(
	q: &mut String,
	anims: &Animations,
	seq: &SequenceDesc,
	first: &AnimDesc,
	weight_lists: &[(String, Vec<f32>)],
) {
	let last_frame = first.frames.saturating_sub(1) as f32;
	let pose = |i: i32| {
		usize::try_from(i)
			.ok()
			.and_then(|i| anims.pose_parameters.get(i))
			.map_or("", |p| p.name.as_str())
	};
	// Without it studiomdl lays the animations out as a square.
	if seq.anims.len() > 1 {
		let _ = writeln!(q, "\tblendwidth {}", seq.group_size[0]);
	}
	for i in 0..2 {
		if seq.param_index[i] >= 0 {
			let _ = writeln!(
				q,
				"\tblend {} {} {}",
				quote(pose(seq.param_index[i])),
				num(seq.param_start[i]),
				num(seq.param_end[i])
			);
		}
	}
	if !seq.activity.is_empty() {
		// studiomdl refuses a weight of 0.
		let comment = if seq.activity_weight == 0 { "// " } else { "" };
		let _ = writeln!(
			q,
			"\t{comment}activity {} {}",
			quote(&seq.activity),
			seq.activity_weight
		);
	}
	for m in &seq.activity_modifiers {
		let _ = writeln!(q, "\tactivitymodifier {}", quote(m));
	}
	for (flag, option) in [
		(anim::STUDIO_LOOPING, "loop"),
		(anim::STUDIO_SNAP, "snap"),
		(anim::STUDIO_AUTOPLAY, "autoplay"),
		(anim::STUDIO_REALTIME, "realtime"),
		(anim::STUDIO_HIDDEN, "hidden"),
		(anim::STUDIO_WORLD, "worldspace"),
	] {
		if seq.flags & flag != 0 {
			let _ = writeln!(q, "\t{option}");
		}
	}
	delta(q, seq.flags);
	if seq.flags & anim::STUDIO_CYCLEPOSE != 0 {
		let _ = writeln!(q, "\tposecycle {}", quote(pose(seq.cycle_pose)));
	}
	if seq.fade_in != DEFAULT_FADE {
		let _ = writeln!(q, "\tfadein {}", num(seq.fade_in));
	}
	if seq.fade_out != DEFAULT_FADE {
		let _ = writeln!(q, "\tfadeout {}", num(seq.fade_out));
	}
	let node = |i: i32| {
		usize::try_from(i - 1)
			.ok()
			.and_then(|i| anims.nodes.get(i))
			.map_or("", String::as_str)
	};
	if seq.entry_node > 0 {
		if seq.entry_node == seq.exit_node {
			let _ = writeln!(q, "\tnode {}", quote(node(seq.entry_node)));
		} else {
			let keyword = if seq.node_flags & 1 != 0 {
				"rtransition"
			} else {
				"transition"
			};
			let _ = writeln!(
				q,
				"\t{keyword} {} {}",
				quote(node(seq.entry_node)),
				quote(node(seq.exit_node))
			);
		}
	}
	for e in &seq.events {
		let frame = (e.cycle * last_frame).round() as i32;
		let name = if e.name.is_empty() {
			e.event.to_string()
		} else {
			e.name.clone()
		};
		let _ = write!(q, "\t{{ event {name} {frame}");
		let options = e.options.trim_matches(char::from(0));
		if !options.is_empty() {
			let _ = write!(q, " {}", quote(options));
		}
		q.push_str(" }\n");
	}
	for l in &seq.layers {
		let Some(target) = usize::try_from(l.sequence)
			.ok()
			.and_then(|i| anims.sequences.get(i))
		else {
			continue;
		};
		// An addlayer onto a one-frame sequence is stored as NaN.
		if l.flags == 0 && (l.start == l.end || l.start.is_nan()) {
			let _ = writeln!(q, "\taddlayer {}", quote(&target.name));
			continue;
		}
		let value = |v: f32| {
			if l.flags & LAYER_POSE != 0 {
				num(v)
			} else {
				num(frame_value(v, last_frame))
			}
		};
		let _ = write!(
			q,
			"\tblendlayer {} {} {} {} {}",
			quote(&target.name),
			value(l.start),
			value(l.peak),
			value(l.tail),
			value(l.end)
		);
		for (flag, option) in [
			(LAYER_XFADE, "xfade"),
			(LAYER_SPLINE, "spline"),
			(LAYER_NOBLEND, "noblend"),
		] {
			if l.flags & flag != 0 {
				let _ = write!(q, " {option}");
			}
		}
		if l.flags & LAYER_POSE != 0 {
			let _ = write!(q, " poseparameter {}", quote(pose(l.pose as i32)));
		}
		if l.flags & LAYER_LOCAL != 0 {
			q.push_str(" local");
		}
		q.push('\n');
	}
	if let Some((name, _)) = weight_lists.iter().find(|(_, w)| *w == seq.weights) {
		let _ = writeln!(q, "\tweightlist {}", quote(name));
	}
	for l in &seq.ik_locks {
		let chain = usize::try_from(l.chain)
			.ok()
			.and_then(|i| anims.ik_chains.get(i))
			.map_or("", |c| c.name.as_str());
		let _ = writeln!(
			q,
			"\tiklock {} {} {}",
			quote(chain),
			num(l.position_weight),
			num(l.local_rotation_weight)
		);
	}
	if let Some(text) = super::key_values(&seq.key_values) {
		let indented: String = text.lines().map(|l| format!("\t{l}\n")).collect();
		let _ = writeln!(q, "\tkeyvalues\n\t{{\n{indented}\t}}");
	}
}

/// One list per distinct set of bone weights that isn't all ones.
fn weight_lists(sequences: &[SequenceDesc]) -> Vec<(String, Vec<f32>)> {
	let mut lists: Vec<(String, Vec<f32>)> = Vec::new();
	for seq in sequences {
		if seq.weights.iter().all(|&w| w == 1.0) || lists.iter().any(|(_, w)| *w == seq.weights) {
			continue;
		}
		lists.push((format!("weights_{}", seq.name), seq.weights.clone()));
	}
	lists
}

fn smd(model: &Model, movements: &[Movement], frames: &[Vec<Pose>]) -> String {
	let mut out = String::from("version 1\nnodes\n");
	for (i, bone) in model.bones.iter().enumerate() {
		let _ = writeln!(out, "{i} {} {}", quote(&bone.name), bone.parent);
	}
	out.push_str("end\nskeleton\n");
	let mut previous: Vec<[f32; 3]> = model.bones.iter().map(|b| b.rotation).collect();
	for (f, frame) in frames.iter().enumerate() {
		let _ = writeln!(out, "time {f}");
		for (i, (bone, pose)) in model.bones.iter().zip(frame).enumerate() {
			let mut pose = *pose;
			if bone.parent < 0 {
				pose = with_movement(pose, movements, f);
				// studiomdl turns what it reads 90° about Z; for animations that lands on the roots.
				pose.position = [pose.position[1], -pose.position[0], pose.position[2]];
				pose.rotation[2] -= std::f32::consts::FRAC_PI_2;
			}
			pose.rotation = closest_angles(pose.rotation, previous[i]);
			previous[i] = pose.rotation;
			let _ = writeln!(out, "{i} {} {}", vec3(pose.position), vec3(pose.rotation));
		}
	}
	out.push_str("end\n");
	out
}

/// The same rotation written nearest to `previous`: studiomdl takes `LZR` turns
/// from the difference of the angles, so they must not jump.
pub(super) fn closest_angles(r: [f32; 3], previous: [f32; 3]) -> [f32; 3] {
	use std::f32::consts::{PI, TAU};
	let near = |v: f32, to: f32| v - ((v - to) / TAU).round() * TAU;
	let flipped = [r[0] + PI, PI - r[1], r[2] + PI];
	let [a, b] = [r, flipped].map(|c| [0, 1, 2].map(|i| near(c[i], previous[i])));
	let distance = |c: [f32; 3]| (0..3).map(|i| (c[i] - previous[i]).powi(2)).sum::<f32>();
	if distance(b) < distance(a) { b } else { a }
}

/// Puts back the motion studiomdl took out of the root, as the engine adds it when playing.
fn with_movement(mut pose: Pose, movements: &[Movement], frame: usize) -> Pose {
	let frame = frame as f32;
	let mut position = [0.0; 3];
	let mut yaw = 0.0;
	let mut previous = 0.0;
	for m in movements {
		let end = m.end_frame as f32;
		if frame <= end {
			let f = if end > previous {
				(frame - previous) / (end - previous)
			} else {
				1.0
			};
			let d = m.v0 * f + 0.5 * (m.v1 - m.v0) * f * f;
			position = [0, 1, 2].map(|i| position[i] + d * m.vector[i]);
			yaw = yaw * (1.0 - f) + m.angle * f;
			break;
		}
		previous = end;
		position = m.position;
		yaw = m.angle;
	}
	if position == [0.0; 3] && yaw == 0.0 {
		return pose;
	}
	let (sin, cos) = yaw.to_radians().sin_cos();
	let p = pose.position;
	pose.position = [
		p[0] * cos - p[1] * sin + position[0],
		p[0] * sin + p[1] * cos + position[1],
		p[2] + position[2],
	];
	pose.rotation[2] += yaw.to_radians();
	pose
}
