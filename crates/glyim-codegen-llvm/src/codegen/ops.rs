use crate::Codegen;
use glyim_hir::HirBinOp;
use inkwell::values::IntValue;
use inkwell::IntPredicate;

pub(crate) fn codegen_binop<'ctx>(
    cg: &Codegen<'ctx>,
    op: HirBinOp,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
) -> Option<IntValue<'ctx>> {
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

fn cmp_extend<'ctx>(
    cg: &Codegen<'ctx>,
    pred: IntPredicate,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
) -> Option<IntValue<'ctx>> {
    let c = cg.builder.build_int_compare(pred, l, r, "cmp").ok()?;
    cg.builder.build_int_z_extend(c, cg.i64_type, "zext").ok()
}

pub(crate) fn codegen_float_binop<'ctx>(
    cg: &Codegen<'ctx>,
    op: &glyim_hir::HirBinOp,
    lhs: inkwell::values::FloatValue<'ctx>,
    rhs: inkwell::values::FloatValue<'ctx>,
) -> Option<inkwell::values::FloatValue<'ctx>> {
    match op {
        glyim_hir::HirBinOp::Add => cg.builder.build_float_add(lhs, rhs, "fadd").ok(),
        glyim_hir::HirBinOp::Sub => cg.builder.build_float_sub(lhs, rhs, "fsub").ok(),
        glyim_hir::HirBinOp::Mul => cg.builder.build_float_mul(lhs, rhs, "fmul").ok(),
        glyim_hir::HirBinOp::Div => cg.builder.build_float_div(lhs, rhs, "fdiv").ok(),
        _ => None,
    }
}

pub(crate) fn codegen_float_cmp<'ctx>(
    cg: &Codegen<'ctx>,
    op: &glyim_hir::HirBinOp,
    lhs: inkwell::values::FloatValue<'ctx>,
    rhs: inkwell::values::FloatValue<'ctx>,
) -> Option<inkwell::values::IntValue<'ctx>> {
    use inkwell::FloatPredicate;
    let pred = match op {
        glyim_hir::HirBinOp::Eq => FloatPredicate::OEQ,
        glyim_hir::HirBinOp::Neq => FloatPredicate::ONE,
        glyim_hir::HirBinOp::Lt => FloatPredicate::OLT,
        glyim_hir::HirBinOp::Gt => FloatPredicate::OGT,
        glyim_hir::HirBinOp::Lte => FloatPredicate::OLE,
        glyim_hir::HirBinOp::Gte => FloatPredicate::OGE,
        _ => return None,
    };
    let c = cg.builder.build_float_compare(pred, lhs, rhs, "fcmp").ok()?;
    cg.builder.build_int_z_extend(c, cg.i64_type, "fcmp_zext").ok()
}
