//! VTA files and the flex, eyeball and mouth options of `$model`.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use super::{File, num, quote};
use crate::mdl::Model;
use crate::mdl::flex::{self, Flexes, MeshFlex};
use crate::mdl::vvd::Vvd;

/// The targets studiomdl gives the two keys of an eyelid.
const LOWERER: f32 = -11.0;
const RAISER: f32 = 11.0;

/// One VTA frame: a flex key with its vertices, gathered from every mesh.
struct Key<'a> {
	flex: &'a MeshFlex,
	/// Vertex in the .vvd, delta, normal delta, side.
	vertices: Vec<(usize, [f32; 3], [f32; 3], u8)>,
}

/// The insides of a `$model` block for each body part whose only submodel has
/// flexes or eyeballs, and the VTA files they use.
pub(super) fn blocks(
	model: &Model,
	flexes: &Flexes,
	vvd: &Vvd,
	body_parts: &[(String, Vec<Option<String>>)],
	files: &mut Vec<File>,
) -> HashMap<usize, String> {
	let mut out = HashMap::new();
	let mut first = true;
	for (p, (_, choices)) in body_parts.iter().enumerate() {
		let [Some(smd)] = choices.as_slice() else {
			continue;
		};
		let (Some(sub), Some(meshes), Some(eyes)) = (
			model.body_parts[p].models.first(),
			flexes.meshes.get(p).and_then(|m| m.first()),
			flexes.eyeballs.get(p).and_then(|e| e.first()),
		) else {
			continue;
		};
		let keys = keys(sub, meshes);
		if keys.is_empty() && eyes.is_empty() && !(first && !flexes.controllers.is_empty()) {
			continue;
		}
		let vta = format!("{}.vta", smd.trim_end_matches(".smd"));
		let mut q = String::new();
		let name = |i: i32| {
			usize::try_from(i)
				.ok()
				.and_then(|i| flexes.descs.get(i))
				.map_or("", String::as_str)
		};
		let bone = |i: i32| {
			usize::try_from(i)
				.ok()
				.and_then(|i| model.bones.get(i))
				.map_or("", |b| b.name.as_str())
		};

		let key_of = |desc: i32, target: usize, value: f32| {
			keys.iter()
				.position(|k| k.flex.desc == desc && k.flex.targets[target] == value)
		};
		let mut eyelid_descs = HashSet::new();
		let mut eye_names = HashSet::new();
		for (i, e) in eyes.iter().enumerate() {
			let mut eye = if !e.name.is_empty() {
				e.name.clone()
			} else if e.z_offset > 0.0 {
				"eye_right".into()
			} else if e.z_offset < 0.0 {
				"eye_left".into()
			} else {
				format!("eye_{i}")
			};
			while !eye_names.insert(eye.clone()) {
				eye.push('_');
			}
			let to_model = model
				.bones
				.get(e.bone.max(0) as usize)
				.map(|b| super::physics::invert(b.pose_to_bone));
			let origin = to_model.map_or(e.origin, |m| super::physics::transform(m, e.origin));
			let material = usize::try_from(e.texture)
				.ok()
				.and_then(|t| model.materials.get(t))
				.map_or("", String::as_str);
			let _ = writeln!(
				q,
				"\teyeball {} {} {} {} {} {} \"iris_unused\" {}",
				quote(&eye),
				quote(bone(e.bone)),
				super::vec3(origin),
				quote(material),
				num(e.radius * 2.0),
				num(e.z_offset.atan().to_degrees()),
				num(if e.iris_scale != 0.0 {
					1.0 / e.iris_scale
				} else {
					1.0
				})
			);
			for lid in [e.upper_lid, e.lower_lid] {
				if lid < 0 || name(lid).is_empty() {
					continue;
				}
				eyelid_descs.insert(lid);
				// The heights are the targets of the lid's two keys: [-11, -10, lowerer, neutral] and
				// [neutral, raiser, 10, 11].
				let lowerer = key_of(lid, 0, LOWERER);
				let raiser = key_of(lid, 3, RAISER);
				let target =
					|k: Option<usize>, i: usize| k.map_or(0.0, |k| keys[k].flex.targets[i]);
				let heights = [target(lowerer, 2), target(lowerer, 3), target(raiser, 1)];
				let split = lid_split(&keys, lid, vvd);
				let _ = writeln!(
					q,
					"\teyelid {} {} lowerer {} {} neutral 0 {} raiser {} {} split {} eyeball {}",
					quote(name(lid)),
					quote(&vta),
					lowerer.map_or(0, |k| k + 1),
					num(heights[0]),
					num(heights[1]),
					raiser.map_or(0, |k| k + 1),
					num(heights[2]),
					num(split),
					quote(&eye)
				);
			}
			for d in e.upper.iter().chain(&e.lower) {
				eyelid_descs.insert(*d);
			}
		}
		if first {
			for (i, m) in flexes.mouths.iter().enumerate() {
				let _ = writeln!(
					q,
					"\tmouth {i} {} {} {}",
					quote(name(m.flex)),
					quote(bone(m.bone)),
					super::vec3(m.forward)
				);
			}
		}

		let mut framed = HashSet::new();
		if !keys.is_empty() {
			let _ = writeln!(
				q,
				"\tflexfile {}\n\t{{\n\t\tdefaultflex frame 0",
				quote(&vta)
			);
			for (i, key) in keys.iter().enumerate() {
				let f = key.flex;
				framed.insert(f.desc);
				if f.pair != 0 {
					framed.insert(f.pair);
				}
				if eyelid_descs.contains(&f.desc) {
					continue;
				}
				let position = if f.targets[1] != 1.0 {
					format!(" position {}", num(f.targets[1]))
				} else {
					String::new()
				};
				if f.pair != 0 {
					let full = name(f.desc);
					let base = &full[..full.len().saturating_sub(1)];
					let _ = writeln!(
						q,
						"\t\tflexpair {} {} frame {}{position}",
						quote(base),
						num(split(key, vvd)),
						i + 1
					);
				} else {
					let _ = writeln!(
						q,
						"\t\tflex {} frame {}{position}",
						quote(name(f.desc)),
						i + 1
					);
				}
			}
			q.push_str("\t}\n");
			files.push(File {
				path: vta.clone(),
				text: vta_text(model, sub, vvd, &keys),
			});
		}

		if first {
			first = false;
			let mut declared: HashSet<i32> = framed.union(&eyelid_descs).copied().collect();
			if let Some(d) = flexes.descs.iter().position(|d| d == "default") {
				declared.insert(d as i32);
			}
			for c in &flexes.controllers {
				let _ = writeln!(
					q,
					"\tflexcontroller {} range {} {} {}",
					c.kind,
					num(c.min),
					num(c.max),
					quote(&c.name)
				);
			}
			for rule in &flexes.rules {
				let used = std::iter::once(rule.flex).chain(
					rule.ops
						.iter()
						.filter(|o| o.op == flex::OP_FETCH2)
						.map(|o| o.value),
				);
				for d in used {
					if declared.insert(d) {
						let _ = writeln!(q, "\tlocalvar {}", name(d));
					}
				}
			}
			for rule in &flexes.rules {
				match expression(&rule.ops, flexes) {
					Some(e) => {
						let _ = writeln!(q, "\t%{} = {e}", name(rule.flex));
					}
					None => {
						let _ = writeln!(q, "\t// the rule for {} has no QC form", name(rule.flex));
					}
				}
			}
		}
		out.insert(p, q);
	}
	out
}

fn keys<'a>(sub: &crate::mdl::SubModel, meshes: &'a [Vec<MeshFlex>]) -> Vec<Key<'a>> {
	let mut keys: Vec<Key> = Vec::new();
	for (mesh, flexes) in sub.meshes.iter().zip(meshes) {
		let first = (sub.first_vertex + mesh.first_vertex) as usize;
		for f in flexes {
			let at = match keys.iter().position(|k| {
				k.flex.desc == f.desc && k.flex.pair == f.pair && k.flex.targets == f.targets
			}) {
				Some(at) => at,
				None => {
					keys.push(Key {
						flex: f,
						vertices: Vec::new(),
					});
					keys.len() - 1
				}
			};
			keys[at].vertices.extend(
				f.vertices
					.iter()
					.map(|v| (first + v.index as usize, v.delta, v.normal_delta, v.side)),
			);
		}
	}
	// studiomdl joins the keys of one flex only when they follow each other.
	let order: Vec<i32> = keys
		.iter()
		.map(|k| k.flex.desc)
		.fold(Vec::new(), |mut seen, d| {
			if !seen.contains(&d) {
				seen.push(d);
			}
			seen
		});
	keys.sort_by(|a, b| {
		let at = |k: &Key| order.iter().position(|&d| d == k.flex.desc);
		at(a)
			.cmp(&at(b))
			.then(a.flex.targets[0].total_cmp(&b.flex.targets[0]))
	});
	keys
}

/// An eyelid keeps the vertices on one side of X: 1 for negative X, -1 for
/// positive, 0 for both, as with one eye in the middle.
fn lid_split(keys: &[Key], lid: i32, vvd: &Vvd) -> f32 {
	let (mut negative, mut positive) = (0usize, 0usize);
	for key in keys.iter().filter(|k| k.flex.desc == lid) {
		for &(i, delta, _, _) in &key.vertices {
			let x = vvd
				.vertices
				.get(i)
				.map_or(0.0, |v| v.position[0] + delta[0]);
			if x < -0.01 {
				negative += 1;
			} else if x > 0.01 {
				positive += 1;
			}
		}
	}
	let total = (negative + positive).max(1);
	if negative * 10 < total {
		-1.0
	} else if positive * 10 < total {
		1.0
	} else {
		0.0
	}
}

/// studiomdl shares a stereo flex out by the X of the moved vertex, with a
/// smooth step over `-split..split`; this undoes it from the vertices in between.
fn split(key: &Key, vvd: &Vvd) -> f32 {
	let mut estimates: Vec<f32> = key
		.vertices
		.iter()
		.filter(|v| v.3 > 0 && v.3 < 255)
		.filter_map(|&(i, delta, _, side)| {
			let x = vvd.vertices.get(i)?.position[0] + delta[0];
			let t = inverse_smooth_step(side as f32 / 255.0);
			let s = 2.0 * t - 1.0;
			(s.abs() > 0.05).then(|| x / s)
		})
		.filter(|s| *s > 0.0)
		.collect();
	if estimates.is_empty() {
		return 0.0;
	}
	estimates.sort_by(f32::total_cmp);
	(estimates[estimates.len() / 2] * 100.0).round() / 100.0
}

/// The `t` in 0..1 for which `3t² - 2t³` is `y`.
pub(super) fn inverse_smooth_step(y: f32) -> f32 {
	let (mut low, mut high) = (0.0f32, 1.0f32);
	for _ in 0..30 {
		let mid = (low + high) / 2.0;
		if 3.0 * mid * mid - 2.0 * mid * mid * mid < y {
			low = mid;
		} else {
			high = mid;
		}
	}
	(low + high) / 2.0
}

fn vta_text(model: &Model, sub: &crate::mdl::SubModel, vvd: &Vvd, keys: &[Key]) -> String {
	let mut out = String::from("version 1\nnodes\n");
	for (i, bone) in model.bones.iter().enumerate() {
		let _ = writeln!(out, "{i} {} {}", quote(&bone.name), bone.parent);
	}
	out.push_str("end\nskeleton\n");
	for f in 0..=keys.len() {
		let _ = writeln!(out, "time {f}");
	}
	out.push_str("end\nvertexanimation\ntime 0\n");
	let first = sub.first_vertex as usize;
	let turn = |p: [f32; 3]| {
		if model.is_static_prop() {
			[p[1], -p[0], p[2]]
		} else {
			p
		}
	};
	let line = |out: &mut String, i: usize, p: [f32; 3], n: [f32; 3]| {
		let _ = writeln!(out, "{i} {} {}", super::vec3(turn(p)), super::vec3(turn(n)));
	};
	for i in first..first + sub.vertices as usize {
		if let Some(v) = vvd.vertices.get(i) {
			line(&mut out, i, v.position, v.normal);
		}
	}
	for (f, key) in keys.iter().enumerate() {
		let _ = writeln!(out, "time {}", f + 1);
		for &(i, delta, normal_delta, _) in &key.vertices {
			if let Some(v) = vvd.vertices.get(i) {
				let p = [0, 1, 2].map(|k| v.position[k] + delta[k]);
				let n = [0, 1, 2].map(|k| v.normal[k] + normal_delta[k]);
				line(&mut out, i, p, n);
			}
		}
	}
	out.push_str("end\n");
	out
}

/// A rule's stack of operations as an infix expression.
pub(super) fn expression(ops: &[flex::Op], flexes: &Flexes) -> Option<String> {
	// Each entry: text and how tightly it binds.
	let mut stack: Vec<(String, u8)> = Vec::new();
	for op in ops {
		match op.op {
			flex::OP_CONST => stack.push((number(op.constant()), 9)),
			flex::OP_FETCH1 => {
				stack.push((flexes.controllers.get(op.value as usize)?.name.clone(), 9))
			}
			flex::OP_FETCH2 => {
				stack.push((format!("%{}", flexes.descs.get(op.value as usize)?), 9))
			}
			flex::OP_ADD | flex::OP_SUB | flex::OP_MUL | flex::OP_DIV => {
				let (b, pb) = stack.pop()?;
				let (a, pa) = stack.pop()?;
				let (symbol, level) = match op.op {
					flex::OP_ADD => ("+", 1),
					flex::OP_SUB => ("-", 1),
					flex::OP_MUL => ("*", 2),
					_ => ("/", 2),
				};
				let a = if pa < level { format!("({a})") } else { a };
				// studiomdl reads left to right, so the right side keeps its brackets at the same level.
				let b = if pb <= level { format!("({b})") } else { b };
				stack.push((format!("{a} {symbol} {b}"), level));
			}
			flex::OP_NEG => {
				let (a, pa) = stack.pop()?;
				let a = if pa < 9 { format!("({a})") } else { a };
				stack.push((format!("-{a}"), 9));
			}
			flex::OP_MAX | flex::OP_MIN => {
				let (b, _) = stack.pop()?;
				let (a, _) = stack.pop()?;
				let f = if op.op == flex::OP_MAX { "max" } else { "min" };
				stack.push((format!("{f}({a}, {b})"), 9));
			}
			// Brackets and commas left in the stream do nothing when it runs.
			10..=12 => {}
			_ => return None,
		}
	}
	match stack.as_slice() {
		[(e, _)] => Some(e.clone()),
		_ => None,
	}
}

fn number(v: f32) -> String {
	let text = num(v);
	let text = text.trim_end_matches('0').trim_end_matches('.');
	if text.is_empty() || text == "-" {
		"0".into()
	} else {
		text.to_owned()
	}
}
