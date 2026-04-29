use crate::codegen::ctx::FunctionContext;
use crate::Codegen;
use glyim_hir::{HirExpr, HirStmt};
use inkwell::values::IntValue;

pub(crate) fn codegen_block<'ctx>(
    cg: &Codegen<'ctx>,
    expr: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    match expr {
        HirExpr::Block { stmts, .. } => {
            let mut last = Some(cg.i64_type.const_int(0, false));
            for stmt in stmts {
                if let Some(v) = codegen_stmt(cg, stmt, fctx) {
                    last = Some(v);
                }
            }
            last
        }
        other => super::expr::codegen_expr(cg, other, fctx),
    }
}

pub(crate) fn codegen_stmt<'ctx>(
    cg: &Codegen<'ctx>,
    stmt: &HirStmt,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    match stmt {
        HirStmt::Let {
            name,
            mutable: _,
            value, ..
        } => {
            let val = super::expr::codegen_expr(cg, value, fctx)?;
            let alloca = cg
                .builder
                .build_alloca(cg.i64_type, cg.interner.resolve(*name))
                .ok()?;
            cg.builder.build_store(alloca, val).ok()?;
            fctx.vars.insert(*name, alloca);
            None
        }
        HirStmt::LetPat { .. } => None,
        HirStmt::Assign { target, value, .. } => {
            let new_val = super::expr::codegen_expr(cg, value, fctx)?;
            if let Some(ptr) = fctx.vars.get(target).copied() {
                cg.builder.build_store(ptr, new_val).ok()?;
            }
            Some(new_val)
        }
        HirStmt::Expr(e) => super::expr::codegen_expr(cg, e, fctx),
    }
}
