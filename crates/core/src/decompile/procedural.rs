//! The `.vrd` file of `$proceduralbones`: helper bones driven by other bones.

use std::fmt::Write;

use super::{File, num, vec3};
use crate::mdl::anim::quaternion_angles;
use crate::mdl::{Model, Procedural};

/// The .vrd, and the QC lines that use it.
pub(super) fn procedural_bones(model: &Model, stem: &str) -> (Option<File>, String) {
	// studiomdl matches .vrd names against bone names without anything up to a dot,
	// and the full name never matches.
	let short = |name: &str| name.rsplit('.').next().unwrap_or(name).to_owned();
	let name = |i: i32| {
		usize::try_from(i)
			.ok()
			.and_then(|i| model.bones.get(i))
			.map_or(String::new(), |b| short(&b.name))
	};
	let parent = |i: i32| {
		usize::try_from(i)
			.ok()
			.and_then(|i| model.bones.get(i))
			.map_or(-1, |b| b.parent)
	};
	let degrees = |q: [f32; 4]| quaternion_angles(q).map(f32::to_degrees);
	let mut vrd = String::new();
	let mut axis = 0;
	for bone in &model.bones {
		match &bone.procedural {
			Some(Procedural::Quaternion { control, triggers }) => {
				let _ = writeln!(
					vrd,
					"<helper> {} {} {} {}\n<basepos> 0 0 0",
					short(&bone.name),
					name(bone.parent),
					name(parent(*control)),
					name(*control)
				);
				for t in triggers {
					let _ = writeln!(
						vrd,
						"<trigger> {} {} {} {}",
						num(t.tolerance.to_degrees()),
						vec3(degrees(t.trigger)),
						vec3(degrees(t.rotation)),
						vec3(t.position)
					);
				}
				vrd.push('\n');
			}
			Some(Procedural::AimAt {
				parent,
				aim,
				at_attachment,
				aim_vector,
				up_vector,
				base,
			}) => {
				let target = if *at_attachment {
					usize::try_from(*aim)
						.ok()
						.and_then(|a| model.attachments.get(a))
						.map_or(String::new(), |a| a.name.clone())
				} else {
					name(*aim)
				};
				let _ = writeln!(
					vrd,
					"<aimconstraint> {} {} {target}\n<aimvector> {}\n<upvector> {}\n<basepos> {}\n",
					short(&bone.name),
					name(*parent),
					vec3(*aim_vector),
					vec3(*up_vector),
					vec3(*base)
				);
			}
			Some(Procedural::Axis { .. }) => axis += 1,
			None => {}
		}
	}
	let mut q = String::new();
	if axis > 0 {
		let _ = writeln!(q, "// {axis} axis interpolation bones are not decompiled.");
	}
	if vrd.is_empty() {
		return (None, q);
	}
	let path = format!("{stem}.vrd");
	let _ = writeln!(q, "$proceduralbones {}", super::quote(&path));
	(Some(File { path, text: vrd }), q)
}
