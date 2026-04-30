use crate::Codegen;
use glyim_hir::HirBinOp;
use inkwell::values::FloatValue;

pub(crate) fn codegen_float_binop<'ctx>(
    cg: &Codegen<'ctx>, op: &HirBinOp,
    lhs: FloatValue<'ctx>, rhs: FloatValue<'ctx>,
) -> Option<FloatValue<'ctx>> {
    match op {
        HirBinOp::Add => cg.builder.build_float_add(lhs, rhs, "fadd").ok(),
        HirBinOp::Sub => cg.builder.build_float_sub(lhs, rhs, "fsub").ok(),
        HirBinOp::Mul => cg.builder.build_float_mul(lhs, rhs, "fmul").ok(),
        HirBinOp::Div => cg.builder.build_float_div(lhs, rhs, "fdiv").ok(),
        _ => None,
    }
}
