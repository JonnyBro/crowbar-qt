//! `$collisionmodel` / `$collisionjoints` and their SMD from a `.phy`.

use std::collections::HashMap;
use std::fmt::Write;

use super::{File, nodes_and_skeleton, num, quote, vec3};
use crate::mdl::Model;
use crate::mdl::phy::Phy;
use crate::vdf::{self, Node, ParseOptions};

/// studiomdl stores a joint's friction scaled down by this.
const JOINT_FRICTION_SCALE: f32 = 0.2;

/// Blocks of the text section that are not studiomdl's own come from `$collisiontext`.
const OWN_BLOCKS: [&str; 5] = [
	"solid",
	"ragdollconstraint",
	"collisionrules",
	"editparams",
	"animatedfriction",
];

/// The collision SMD and the QC lines that use it.
pub(super) fn physics(model: &Model, phy: &Phy, stem: &str) -> (File, String) {
	let root = vdf::parse(&phy.text, ParseOptions::RAW).unwrap_or_else(|_| Node {
		key: String::new(),
		value: vdf::Value::Object(Vec::new()),
	});
	let blocks = |key: &'static str| {
		root.children()
			.iter()
			.filter(move |n| n.key.eq_ignore_ascii_case(key))
	};
	let solids: Vec<&Node> = blocks("solid").collect();
	let edit = blocks("editparams").next();
	let constraints: Vec<&Node> = blocks("ragdollconstraint").collect();
	let joints = phy.solids.len() > 1 || !constraints.is_empty();
	let bone_index = |name: &str| {
		model
			.bones
			.iter()
			.position(|b| b.name.eq_ignore_ascii_case(name))
	};
	let solid_name = |index: &str| {
		solids
			.iter()
			.find(|s| s.get("index") == Some(index))
			.and_then(|s| s.get("name"))
			.unwrap_or("")
	};

	let path = format!("{stem}_physics.smd");
	let mut smd = String::new();
	nodes_and_skeleton(&mut smd, &model.bones, false);
	smd.push_str("triangles\n");
	for (i, solid) in phy.solids.iter().enumerate() {
		let named = solids
			.get(i)
			.and_then(|s| s.get("name"))
			.and_then(bone_index);
		for piece in &solid.pieces {
			let bone = usize::try_from(piece.bone).ok().or(named);
			let to_model = bone
				.and_then(|b| model.bones.get(b))
				.map(|b| invert(b.pose_to_bone));
			let points: Vec<[f32; 3]> = piece
				.points
				.iter()
				.map(|&p| {
					let p = to_model.map_or(p, |m| transform(m, p));
					if model.is_static_prop() {
						[p[1], -p[0], p[2]]
					} else {
						p
					}
				})
				.collect();
			// studiomdl tells convex pieces apart by shared vertices, so each point
			// gets one normal, out from the piece's centre, wherever it is used.
			let centre = points
				.iter()
				.fold([0.0; 3], |acc, p| {
					[acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]]
				})
				.map(|v| v / points.len().max(1) as f32);
			let bone = bone.unwrap_or(0);
			for tri in &piece.triangles {
				smd.push_str("phy\n");
				for &i in tri {
					let p = points.get(i as usize).copied().unwrap_or_default();
					let _ = writeln!(
						smd,
						"{bone} {} {} 0 0 1 {bone} 1",
						vec3(p),
						vec3(normalize(sub(p, centre)))
					);
				}
			}
		}
	}
	smd.push_str("end\n");

	let mut q = String::new();
	let command = if joints {
		"$collisionjoints"
	} else {
		"$collisionmodel"
	};
	let _ = writeln!(q, "{command} {}\n{{", quote(&path));
	if let Some(mass) = edit.and_then(|e| e.get("totalmass")) {
		let _ = writeln!(q, "\t$mass {mass}");
	}
	for key in ["inertia", "damping", "rotdamping"] {
		if let Some(value) = most_common(solids.iter().filter_map(|s| s.get(key))) {
			let _ = writeln!(q, "\t${key} {value}");
		}
	}
	if let Some(root) = edit
		.and_then(|e| e.get("rootname"))
		.filter(|r| !r.trim().is_empty())
	{
		let _ = writeln!(q, "\t$rootbone {}", quote(root));
	}
	if edit.and_then(|e| e.get("concave")) == Some("1") {
		let pieces: usize = phy.solids.iter().map(|s| s.pieces.len()).sum();
		let _ = writeln!(q, "\t$concave\n\t$maxconvexpieces {pieces}");
	}
	for merge in edit
		.map(|e| e.children())
		.unwrap_or_default()
		.iter()
		.filter(|n| n.key.eq_ignore_ascii_case("jointmerge"))
		.filter_map(Node::as_str)
	{
		if let Some((a, b)) = merge.split_once(',') {
			let _ = writeln!(q, "\t$jointmerge {} {}", quote(a), quote(b));
		}
	}
	if joints {
		joint_lines(&mut q, &solids);
		for c in &constraints {
			let child = solid_name(c.get("child").unwrap_or(""));
			for axis in ["x", "y", "z"] {
				let value = |k: &str| c.get(&format!("{axis}{k}")).unwrap_or("0");
				let friction =
					value("friction").parse::<f32>().unwrap_or(0.0) / JOINT_FRICTION_SCALE;
				let _ = writeln!(
					q,
					"\t$jointconstrain {} {axis} limit {} {} {}",
					quote(child),
					value("min"),
					value("max"),
					num(friction)
				);
			}
		}
		if let Some(rules) = blocks("collisionrules").next() {
			if rules.get("selfcollisions") == Some("0") {
				q.push_str("\t$noselfcollisions\n");
			}
			// Stored last first.
			for pair in rules
				.children()
				.iter()
				.rev()
				.filter(|n| n.key.eq_ignore_ascii_case("collisionpair"))
				.filter_map(Node::as_str)
			{
				if let Some((a, b)) = pair.split_once(',') {
					let _ = writeln!(
						q,
						"\t$jointcollide {} {}",
						quote(solid_name(a.trim())),
						quote(solid_name(b.trim()))
					);
				}
			}
		}
		if let Some(f) = blocks("animatedfriction").next() {
			let v = |k: &str| f.get(k).unwrap_or("0");
			let _ = writeln!(
				q,
				"\t$animatedfriction {} {} {} {} {}",
				v("animfrictionmin"),
				v("animfrictionmax"),
				v("animfrictiontimein"),
				v("animfrictiontimehold"),
				v("animfrictiontimeout")
			);
		}
	}
	q.push_str("}\n");

	let extra: Vec<&Node> = root
		.children()
		.iter()
		.filter(|n| !OWN_BLOCKS.iter().any(|b| n.key.eq_ignore_ascii_case(b)))
		.collect();
	if !extra.is_empty() {
		q.push_str("$collisiontext\n{\n");
		for node in extra {
			super::write_node(&mut q, node, 1);
		}
		q.push_str("}\n");
	}
	(File { path, text: smd }, q)
}

/// Per-joint values where a solid differs from the rest; masses come back as
/// the bias studiomdl spread the total by.
fn joint_lines(q: &mut String, solids: &[&Node]) {
	let float = |s: &Node, k: &str| s.get(k).and_then(|v| v.parse::<f64>().ok());
	let total_mass: f64 = solids.iter().filter_map(|s| float(s, "mass")).sum();
	let total_volume: f64 = solids.iter().filter_map(|s| float(s, "volume")).sum();
	for s in solids {
		let name = s.get("name").unwrap_or("");
		if let (Some(mass), Some(volume)) = (float(s, "mass"), float(s, "volume"))
			&& total_mass > 0.0
			&& volume > 0.0
		{
			let bias = (mass / total_mass) / (volume / total_volume);
			if (bias - 1.0).abs() > 0.01 {
				let _ = writeln!(q, "\t$jointmassbias {} {}", quote(name), num(bias as f32));
			}
		}
	}
	for key in ["inertia", "damping", "rotdamping"] {
		let common = most_common(solids.iter().filter_map(|s| s.get(key)));
		for s in solids {
			if let Some(value) = s.get(key).filter(|v| Some(*v) != common) {
				let name = s.get("name").unwrap_or("");
				let _ = writeln!(q, "\t$joint{key} {} {value}", quote(name));
			}
		}
	}
}

fn most_common<'a>(values: impl Iterator<Item = &'a str>) -> Option<&'a str> {
	let mut counts: HashMap<&str, usize> = HashMap::new();
	let mut order = Vec::new();
	for v in values {
		let n = counts.entry(v).or_default();
		if *n == 0 {
			order.push(v);
		}
		*n += 1;
	}
	// The first of the most frequent, so ties come out the same every time.
	order
		.into_iter()
		.fold(None, |best: Option<&str>, v| match best {
			Some(b) if counts[b] >= counts[v] => Some(b),
			_ => Some(v),
		})
}

/// A rigid 3×4 matrix's inverse.
fn invert(m: [[f32; 4]; 3]) -> [[f32; 4]; 3] {
	let mut out = [[0.0; 4]; 3];
	for r in 0..3 {
		for c in 0..3 {
			out[r][c] = m[c][r];
		}
		out[r][3] = -(0..3).map(|k| m[k][r] * m[k][3]).sum::<f32>();
	}
	out
}

fn transform(m: [[f32; 4]; 3], p: [f32; 3]) -> [f32; 3] {
	[0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
	[a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
	let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
	if len > 0.0 {
		v.map(|x| x / len)
	} else {
		[0.0, 0.0, 1.0]
	}
}
