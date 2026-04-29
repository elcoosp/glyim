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
    // Set debug location for the statement
    let span = match stmt {
        HirStmt::Let { span, .. } => *span,
        HirStmt::LetPat { span, .. } => *span,
        HirStmt::Assign { span, .. } => *span,
        HirStmt::Expr(e) => e.get_span(),
    };
    cg.set_debug_location_for_span(span);

    match stmt {
        HirStmt::Let {
            name,
            mutable: _,
            value,
            span,
        } => {
            let val = super::expr::codegen_expr(cg, value, fctx)?;
            let alloca = cg
                .builder
                .build_alloca(cg.i64_type, cg.interner.resolve(*name))
                .ok()?;
            cg.builder.build_store(alloca, val).ok()?;

            // Emit DILocalVariable + llvm.dbg.declare
            if let (Some(ref di), Some(ref src), Some(sp)) =
                (&cg.debug_info, &cg.source_str, &cg.current_subprogram)
            {
                let line = crate::debug::DebugInfoGen::byte_offset_to_line(src, span.start);
                let resolved_name = cg.interner.resolve(*name);
                if let Ok(var) = di.create_local_variable(resolved_name, *sp, line) {
                    if let Ok(loc) = di.create_location(*sp, line, 0) {
                        let _ = di.insert_declare(&cg.builder, &cg.module, var, alloca, loc);
                    }
                }
            }

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
