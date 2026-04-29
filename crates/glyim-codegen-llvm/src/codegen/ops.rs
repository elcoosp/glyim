use crate::Codegen;
use glyim_hir::HirBinOp;
use inkwell::IntPredicate;
use inkwell::values::IntValue;

pub(crate) fn codegen_binop<'ctx>(cg: &Codegen<'ctx>, op: HirBinOp, l: IntValue<'ctx>, r: IntValue<'ctx>) -> Option<IntValue<'ctx>> {
    match op {
        HirBinOp::Add => cg.builder.build_int_add(l, r, "add").ok(),
        HirBinOp::Sub => cg.builder.build_int_sub(l, r, "sub").ok(),
        HirBinOp::Mul => cg.builder.build_int_mul(l, r, "mul").ok(),
        HirBinOp::Div => cg.builder.build_int_signed_div(l, r, "div").ok(),
        HirBinOp::Mod => cg.builder.build_int_signed_rem(l, r, "rem").ok(),
        HirBinOp::Eq => cmp_extend(cg, IntPredicate::EQ, l, r),
        HirBinOp::Neq => cmp_extend(cg, IntPredicate::NE, l, r),
        HirBinOp::Lt => cmp_extend(cg, IntPredicate::SLT, l, r),
        HirBinOp::Gt => cmp_extend(cg, IntPredicate::SGT, l, r),
        HirBinOp::Lte => cmp_extend(cg, IntPredicate::SLE, l, r),
        HirBinOp::Gte => cmp_extend(cg, IntPredicate::SGE, l, r),
        HirBinOp::And => cg.builder.build_and(l, r, "and").ok(),
        HirBinOp::Or => cg.builder.build_or(l, r, "or").ok(),
    }
}

fn cmp_extend<'ctx>(cg: &Codegen<'ctx>, pred: IntPredicate, l: IntValue<'ctx>, r: IntValue<'ctx>) -> Option<IntValue<'ctx>> {
    let c = cg.builder.build_int_compare(pred, l, r, "cmp").ok()?;
    cg.builder.build_int_z_extend(c, cg.i64_type, "zext").ok()
}
