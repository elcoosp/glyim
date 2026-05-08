use crate::elab::ElabContext;
use crate::ty::{Ty, TyKind};
use glyim_hir::{HirExpr, HirItem};

pub fn synth_expr(ctx: &mut ElabContext, expr: &HirExpr) -> Ty {
    let ty = match expr {
        HirExpr::IntLit { id, .. } => {
            let ty = ctx.arena.alloc(TyKind::Int);
            ctx.record_type(*id, ty);
            ty
        }
        HirExpr::BoolLit { id, .. } => {
            let ty = ctx.arena.alloc(TyKind::Bool);
            ctx.record_type(*id, ty);
            ty
        }
        HirExpr::StrLit { id, .. } => {
            let ty = ctx.arena.alloc(TyKind::Str);
            ctx.record_type(*id, ty);
            ty
        }
        HirExpr::FloatLit { id, .. } => {
            let ty = ctx.arena.alloc(TyKind::Float);
            ctx.record_type(*id, ty);
            ty
        }
        HirExpr::UnitLit { id, .. } => {
            let ty = ctx.arena.alloc(TyKind::Unit);
            ctx.record_type(*id, ty);
            ty
        }
        HirExpr::Ident { id, name, .. } => {
            let ty = ctx.scope.lookup(*name).unwrap_or_else(|| {
                ctx.arena.alloc(TyKind::Error)
            });
            ctx.record_type(*id, ty);
            ty
        }
        HirExpr::Binary { id, .. } => {
            let result_ty = ctx.arena.alloc(TyKind::Int);
            ctx.record_type(*id, result_ty);
            result_ty
        }
        HirExpr::Call { id, callee, args, .. } => {
            let arg_tys: Vec<Ty> = args.iter().map(|a| synth_expr(ctx, a)).collect();

            // Real function lookup from HIR
            if let Some(fn_def) = lookup_fn(ctx, *callee) {
                if !fn_def.type_params.is_empty() && arg_tys.len() >= fn_def.type_params.len() {
                    let concrete_args: Vec<Ty> = arg_tys[..fn_def.type_params.len()].to_vec();
                    ctx.call_type_args.insert(*id, concrete_args);
                }
            }

            let result_ty = ctx.arena.fresh_infer(glyim_diag::Span::new(0, 0));
            ctx.record_type(*id, result_ty);
            result_ty
        }
        HirExpr::MethodCall { id, receiver, args, .. } => {
            let recv_ty = synth_expr(ctx, receiver);
            let _arg_tys: Vec<Ty> = args.iter().map(|a| synth_expr(ctx, a)).collect();

            if let TyKind::App(_, type_args) = ctx.arena.get(recv_ty) {
                if !type_args.is_empty() {
                    ctx.call_type_args.insert(*id, type_args.clone());
                }
            }

            let result_ty = ctx.arena.fresh_infer(glyim_diag::Span::new(0, 0));
            ctx.record_type(*id, result_ty);
            result_ty
        }
        HirExpr::StructLit { id, struct_name, fields, .. } => {
            for (_, field_expr) in fields {
                synth_expr(ctx, field_expr);
            }
            let ty = ctx.arena.alloc(TyKind::Named(*struct_name));
            ctx.record_type(*id, ty);
            ty
        }
        HirExpr::FieldAccess { id, object, .. } => {
            let _obj_ty = synth_expr(ctx, object);
            let field_ty = ctx.arena.fresh_infer(glyim_diag::Span::new(0, 0));
            ctx.record_type(*id, field_ty);
            field_ty
        }
        HirExpr::Match { id, scrutinee, arms, .. } => {
            synth_expr(ctx, scrutinee);
            let mut arm_tys = Vec::new();
            for arm in arms {
                // Push new scope for pattern bindings
                ctx.scope.push_child();
                arm_tys.push(synth_expr(ctx, &arm.body));
                // Restore parent scope
                ctx.scope.pop_child();
            }
            let result_ty = arm_tys.first().copied().unwrap_or_else(|| ctx.arena.alloc(TyKind::Unit));
            ctx.record_type(*id, result_ty);
            result_ty
        }
        HirExpr::Block { id, stmts, .. } => {
            let mut last_ty = ctx.arena.alloc(TyKind::Unit);
            for stmt in stmts {
                if let glyim_hir::HirStmt::Expr(e) = stmt {
                    last_ty = synth_expr(ctx, e);
                } else if let glyim_hir::HirStmt::Let { name, value, .. } = stmt {
                    let val_ty = synth_expr(ctx, value);
                    ctx.scope.insert(*name, val_ty);
                }
            }
            ctx.record_type(*id, last_ty);
            last_ty
        }
        _ => {
            let ty = ctx.arena.fresh_infer(glyim_diag::Span::new(0, 0));
            ctx.record_type(expr.get_id(), ty);
            ty
        }
    };
    ty
}

/// Look up a function definition in the HIR, returning its type parameters.
fn lookup_fn(ctx: &ElabContext, callee: glyim_interner::Symbol) -> Option<FnSigInfo> {
    for item in ctx.hir_items {
        match item {
            HirItem::Fn(f) if f.name == callee => {
                return Some(FnSigInfo {
                    type_params: f.type_params.clone(),
                });
            }
            HirItem::Impl(imp) => {
                for method in &imp.methods {
                    if method.name == callee {
                        return Some(FnSigInfo {
                            type_params: method.type_params.clone(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    None
}

struct FnSigInfo {
    type_params: Vec<glyim_interner::Symbol>,
}
