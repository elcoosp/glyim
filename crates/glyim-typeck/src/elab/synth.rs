use crate::elab::ElabContext;
use crate::ty::{Ty, TyKind};
use glyim_hir::HirExpr;

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
        _ => {
            let ty = ctx.arena.alloc(TyKind::Error);
            ty
        }
    };
    ty
}
