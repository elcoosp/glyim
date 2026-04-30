mod control;
mod data;
mod float_ops;

use crate::codegen::ctx::FunctionContext;
use crate::Codegen;
use glyim_hir::{HirExpr, HirUnOp};
use inkwell::types::BasicType;
use inkwell::values::IntValue;

pub(crate) fn codegen_expr<'ctx>(
    cg: &Codegen<'ctx>,
    expr: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    cg.set_debug_location_for_span(expr.get_span());
    match expr {
        HirExpr::IntLit { value: n, .. } => Some(cg.i64_type.const_int(*n as u64, true)),
        HirExpr::Ident { name: sym, .. } => {
            let ptr = fctx.vars.get(sym)?;
            cg.builder
                .build_load(cg.i64_type, *ptr, cg.interner.resolve(*sym))
                .ok()
                .map(|v| v.into_int_value())
        }
        HirExpr::Binary { op, lhs, rhs, .. } => {
            let l = codegen_expr(cg, lhs, fctx)?;
            let r = codegen_expr(cg, rhs, fctx)?;
            super::ops::codegen_binop(cg, op.clone(), l, r)
        }
        HirExpr::Unary { op, operand, .. } => {
            let val = codegen_expr(cg, operand, fctx)?;
            match op {
                HirUnOp::Neg => {
                    let zero = cg.i64_type.const_int(0, false);
                    cg.builder.build_int_sub(zero, val, "neg").ok()
                }
                HirUnOp::Not => cg.builder.build_not(val, "not").ok(),
            }
        }
        HirExpr::BoolLit { value: b, .. } => {
            let i1 = cg
                .context
                .bool_type()
                .const_int(if *b { 1 } else { 0 }, false);
            Some(
                cg.builder
                    .build_int_z_extend(i1, cg.i64_type, "bool_zext")
                    .ok()?,
            )
        }
        HirExpr::UnitLit { .. } => Some(cg.i64_type.const_int(0, false)),
        HirExpr::StrLit { value: s, .. } => super::string::codegen_string_literal(cg, s),
        HirExpr::SizeOf { target_type, .. } => {
            if let Some(llvm_type) = cg.hir_type_to_llvm(target_type) {
                Some(
                    llvm_type
                        .size_of()
                        .unwrap_or_else(|| cg.i64_type.const_int(0, false)),
                )
            } else {
                Some(cg.i64_type.const_int(0, false))
            }
        }
        HirExpr::Println { arg, .. } => super::string::codegen_println(cg, arg, fctx),
        HirExpr::Assert {
            condition, message, ..
        } => super::string::codegen_assert(cg, condition, message, fctx),
        HirExpr::Call { callee, args, .. } => super::string::codegen_call(cg, callee, args, fctx),
        HirExpr::Block { stmts, .. } => {
            let mut last = Some(cg.i64_type.const_int(0, false));
            for stmt in stmts {
                if let Some(v) = super::stmt::codegen_stmt(cg, stmt, fctx) {
                    last = Some(v);
                }
            }
            last
        }
        HirExpr::If { .. } => control::codegen_if(cg, expr, fctx),
        HirExpr::Match { .. } => control::codegen_match(cg, expr, fctx),
        HirExpr::StructLit { .. } => data::codegen_struct_lit(cg, expr, fctx),
        HirExpr::EnumVariant { .. } => data::codegen_enum_variant(cg, expr, fctx),
        HirExpr::FieldAccess { .. } => data::codegen_field_access(cg, expr, fctx),
        HirExpr::TupleLit { .. } => data::codegen_tuple_lit(cg, expr, fctx),
        HirExpr::Return { value, .. } => {
            let ret_val = match value {
                Some(v) => codegen_expr(cg, v, fctx)?,
                None => cg.i64_type.const_int(0, false),
            };
            cg.builder.build_return(Some(&ret_val)).ok()?;
            None
        }
        HirExpr::As { .. } => Some(cg.i64_type.const_int(0, false)),
        HirExpr::FloatLit { value: f, .. } => {
            let fv = cg.f64_type.const_float(*f);
            let alloca = cg.builder.build_alloca(cg.f64_type, "float_tmp").ok()?;
            cg.builder.build_store(alloca, fv).ok()?;
            Some(
                cg.builder
                    .build_ptr_to_int(alloca, cg.i64_type, "f2i64")
                    .ok()?,
            )
        }
    }
}
