use crate::ty::{Ty, TyArena, TyKind};
use crate::unify::UnificationTable;
use glyim_hir::types::ExprId;
use glyim_hir::types::HirType;
use std::collections::HashMap;

/// Resolve a Ty to a concrete HirType, following unification chains.
pub fn resolve_ty(arena: &TyArena, unification: &UnificationTable, ty: Ty) -> HirType {
    let ty = unification.find(arena, ty);
    match arena.get(ty) {
        TyKind::Int => HirType::Int,
        TyKind::Float => HirType::Float,
        TyKind::Bool => HirType::Bool,
        TyKind::Str => HirType::Str,
        TyKind::Unit => HirType::Unit,
        TyKind::Never => HirType::Never,
        TyKind::Error => HirType::Error,
        TyKind::Infer => HirType::Error, // unresolved hole
        TyKind::Named(sym) => HirType::Named(*sym),
        TyKind::App(sym, args) => HirType::Generic(
            *sym,
            args.iter()
                .map(|&a| resolve_ty(arena, unification, a))
                .collect(),
        ),
        TyKind::Fn(params, ret) => HirType::Func(
            params
                .iter()
                .map(|&p| resolve_ty(arena, unification, p))
                .collect(),
            Box::new(resolve_ty(arena, unification, *ret)),
        ),
        TyKind::RawPtr(inner) => HirType::RawPtr(Box::new(resolve_ty(arena, unification, *inner))),
        TyKind::Code(_) => HirType::Error,
        TyKind::Const(_, _) => HirType::Error,
        TyKind::EffectFn(params, ret, _) => HirType::Func(
            params
                .iter()
                .map(|&p| resolve_ty(arena, unification, p))
                .collect(),
            Box::new(resolve_ty(arena, unification, *ret)),
        ),
        TyKind::Any => HirType::Error,
        TyKind::TypeInfo(_) => HirType::Error,
    }
}

/// Convert a whole expression type map into HirType vector.
pub fn resolve_expr_types(
    arena: &TyArena,
    unification: &UnificationTable,
    elab_map: &HashMap<ExprId, Ty>,
) -> Vec<HirType> {
    let max_id = elab_map.keys().map(|id| id.as_usize()).max().unwrap_or(0);
    let mut expr_types = vec![HirType::Error; max_id + 1];
    for (&id, &ty) in elab_map {
        expr_types[id.as_usize()] = resolve_ty(arena, unification, ty);
    }
    expr_types
}
