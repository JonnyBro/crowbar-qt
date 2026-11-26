use super::*;
use crate::mdl::anim::tests::Builder;

#[test]
fn reads_descs_controllers_rules_and_mouths() {
	let mut b = Builder::new(1000);
	b.i32(260, 2);
	b.i32(264, 500);
	b.name(500, 500, "AU1R");
	b.name(504, 504, "mouth");
	b.i32(268, 1);
	b.i32(272, 520);
	b.name(520, 520, "brow");
	b.name(520, 524, "right_inner_raiser");
	b.f32(532, 0.0);
	b.f32(536, 2.0);
	// %AU1R = right_inner_raiser * 0.5
	b.i32(276, 1);
	b.i32(280, 560);
	b.i32(564, 3);
	b.i32(568, 12);
	b.i32(572, OP_FETCH1);
	b.i32(576, 0);
	b.i32(580, OP_CONST);
	b.f32(584, 0.5);
	b.i32(588, OP_MUL);
	b.i32(292, 1);
	b.i32(296, 620);
	b.i32(620, 3);
	b.f32(628, 1.0);
	b.i32(636, 1);
	let data = b.finish();

	let f = parse(&data).unwrap();
	assert_eq!(f.descs, ["AU1R", "mouth"]);
	assert_eq!(f.controllers[0].kind, "brow");
	assert_eq!(f.controllers[0].name, "right_inner_raiser");
	assert_eq!(f.controllers[0].max, 2.0);
	let ops: Vec<i32> = f.rules[0].ops.iter().map(|o| o.op).collect();
	assert_eq!(ops, [OP_FETCH1, OP_CONST, OP_MUL]);
	assert_eq!(f.rules[0].ops[1].constant(), 0.5);
	assert_eq!(f.mouths[0].bone, 3);
	assert_eq!(f.mouths[0].forward, [0.0, 1.0, 0.0]);
	assert_eq!(f.mouths[0].flex, 1);
}
