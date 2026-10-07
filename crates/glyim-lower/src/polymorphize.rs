//! Polymorphization: detect unused generic parameters and avoid
//! monomorphizing over them, reducing code size.
//!
//! Follows rustc's `-Zpolymorphize` design:
//! - Analyze which generic parameters are used in a function's MIR body
//! - Replace unused parameters with a canonical placeholder (unit type)
//! - Deduplicate mono items that differ only in unused parameters

use glyim_core::arena::IndexVec;
use glyim_mir::{
    self, AggregateKind, LocalDecl, LocalIdx, MirConstKind, Operand, ProjectionElem, Rvalue,
    StatementKind, TerminatorKind,
};
use glyim_type::*;
use std::collections::HashSet;

use crate::mono::{MonoItem, MonoItemData};

/// analyze_used_params.
pub fn analyze_used_params(
    body: &glyim_mir::Body,
    ctx: &dyn TypeLookup,
    substs: Substitution,
) -> Vec<bool> {
    let n = ctx.substitution_args(substs).len();
    let mut used = vec![false; n];

    for local in body.locals.iter() {
        mark_used_params(local.ty, ctx, &mut used);
    }

    for block in body.basic_blocks.iter() {
        for stmt in &block.statements {
            if let StatementKind::Assign(_, ref rvalue) = stmt.kind {
                mark_used_params_in_rvalue(rvalue, &body.locals, ctx, &mut used);
            }
        }
        mark_used_params_in_terminator(&block.terminator.kind, &body.locals, ctx, &mut used);
    }

    used
}

/// polymorphize_substs.
pub fn polymorphize_substs(
    ctx: &mut TyCtxMut,
    substs: Substitution,
    used: &[bool],
) -> Substitution {
    let args: Vec<GenericArg> = ctx
        .substitution_args(substs)
        .iter()
        .enumerate()
        .map(|(i, arg)| {
            if i < used.len() && !used[i] {
                match arg {
                    GenericArg::Ty(_) => GenericArg::Ty(ctx.unit_ty()),
                    GenericArg::Lifetime(r) => GenericArg::Lifetime(r.clone()),
                    GenericArg::Const(_) => GenericArg::Const(Const {
                        kind: ConstKind::Unit,
                        ty: ctx.unit_ty(),
                    }),
                }
            } else {
                arg.clone()
            }
        })
        .collect();
    ctx.intern_substitution(args)
}

/// compute_poly_item.
pub fn compute_poly_item(ctx: &mut TyCtxMut, item: &MonoItem, body: &glyim_mir::Body) -> MonoItem {
    match item {
        MonoItem::Fn { def_id, substs } => {
            if substs.is_empty() {
                return item.clone();
            }
            let used = analyze_used_params(body, ctx, *substs);
            let poly_substs = polymorphize_substs(ctx, *substs, &used);
            MonoItem::Fn {
                def_id: *def_id,
                substs: poly_substs,
            }
        }
        MonoItem::Const { def_id, substs } => {
            if substs.is_empty() {
                return item.clone();
            }
            let used = analyze_used_params(body, ctx, *substs);
            let poly_substs = polymorphize_substs(ctx, *substs, &used);
            MonoItem::Const {
                def_id: *def_id,
                substs: poly_substs,
            }
        }
        MonoItem::Static { .. } => item.clone(),
        MonoItem::DropGlue { ty } => {
            // Handle each variant separately to avoid holding a reference
            // across mutable borrows.
            match ctx.ty_kind(*ty) {
                TyKind::Adt(adt_id, substs) => {
                    if substs.is_empty() {
                        return item.clone();
                    }
                    let adt_id = *adt_id;
                    let substs = *substs;
                    let used = analyze_used_params(body, ctx, substs);
                    let poly_substs = polymorphize_substs(ctx, substs, &used);
                    let new_ty = ctx.mk_ty(TyKind::Adt(adt_id, poly_substs));
                    MonoItem::DropGlue { ty: new_ty }
                }
                TyKind::Tuple(substs) => {
                    if substs.is_empty() {
                        return item.clone();
                    }
                    let substs = *substs;
                    let used = analyze_used_params(body, ctx, substs);
                    let poly_substs = polymorphize_substs(ctx, substs, &used);
                    let new_ty = ctx.mk_ty(TyKind::Tuple(poly_substs));
                    MonoItem::DropGlue { ty: new_ty }
                }
                TyKind::Closure(id, substs) => {
                    if substs.is_empty() {
                        return item.clone();
                    }
                    let id = *id;
                    let substs = *substs;
                    let used = analyze_used_params(body, ctx, substs);
                    let poly_substs = polymorphize_substs(ctx, substs, &used);
                    let new_ty = ctx.mk_ty(TyKind::Closure(id, poly_substs));
                    MonoItem::DropGlue { ty: new_ty }
                }
                TyKind::FnDef(id, substs) => {
                    if substs.is_empty() {
                        return item.clone();
                    }
                    let id = *id;
                    let substs = *substs;
                    let used = analyze_used_params(body, ctx, substs);
                    let poly_substs = polymorphize_substs(ctx, substs, &used);
                    let new_ty = ctx.mk_ty(TyKind::FnDef(id, poly_substs));
                    MonoItem::DropGlue { ty: new_ty }
                }
                TyKind::Opaque(id, substs) => {
                    if substs.is_empty() {
                        return item.clone();
                    }
                    let id = *id;
                    let substs = *substs;
                    let used = analyze_used_params(body, ctx, substs);
                    let poly_substs = polymorphize_substs(ctx, substs, &used);
                    let new_ty = ctx.mk_ty(TyKind::Opaque(id, poly_substs));
                    MonoItem::DropGlue { ty: new_ty }
                }
                _ => item.clone(),
            }
        }
    }
}

/// deduplicate.
///
/// T101-PATCHED [LOW-10]: prefer the pre-substitution body (via
/// `pre_subst`) for `analyze_used_params`. The substituted body in
/// `MonoItemData::body` has no `TyKind::Param`, so the analysis would
/// always come back all-false and dedup would merge every instantiation
/// of the same generic fn — a wrong-code bug. When `pre_subst` is None
/// (unit tests / standalone callers), fall back to the substituted body
/// (preserving the previous behavior so existing tests keep passing).
pub fn deduplicate(ctx: &mut TyCtxMut, items: &[MonoItemData]) -> Vec<MonoItemData> {
    deduplicate_with_pre_subst(ctx, items, None)
}

/// T101-PATCHED [LOW-10]: dedup with an optional pre-substitution body map.
pub fn deduplicate_with_pre_subst(
    ctx: &mut TyCtxMut,
    items: &[MonoItemData],
    pre_subst: Option<&std::collections::HashMap<glyim_core::def_id::DefId, std::sync::Arc<glyim_mir::Body>>>,
) -> Vec<MonoItemData> {
    let mut seen: HashSet<MonoItem> = HashSet::new();
    let mut result = Vec::new();

    for data in items {
        // Look up the pre-subst body for this item (if the map was
        // supplied). The DefId key is (CrateId(0), LocalDefId) for
        // Fn/Const/Static; DropGlue has no underlying source body so
        // uses its constructed body directly.
        let pre_body = pre_subst.and_then(|m| {
            let key = match &data.item {
                MonoItem::Fn { def_id, .. } => Some(glyim_core::def_id::DefId::new(
                    glyim_core::def_id::CrateId::from_raw(0),
                    glyim_core::def_id::LocalDefId::from_raw(def_id.to_raw()),
                )),
                MonoItem::Const { def_id, .. } => Some(glyim_core::def_id::DefId::new(
                    glyim_core::def_id::CrateId::from_raw(0),
                    glyim_core::def_id::LocalDefId::from_raw(def_id.to_raw()),
                )),
                MonoItem::Static { def_id } => Some(glyim_core::def_id::DefId::new(
                    glyim_core::def_id::CrateId::from_raw(0),
                    glyim_core::def_id::LocalDefId::from_raw(def_id.to_raw()),
                )),
                MonoItem::DropGlue { .. } => None,
            };
            key.and_then(|k| m.get(&k).cloned())
        });

        let analysis_body: &glyim_mir::Body = pre_body.as_deref().unwrap_or(&data.body);
        let poly_item = compute_poly_item(ctx, &data.item, analysis_body);
        if seen.contains(&poly_item) {
            continue;
        }
        seen.insert(poly_item.clone());
        result.push(MonoItemData {
            item: poly_item,
            body: data.body.clone(),
            symbol: data.symbol.clone(),
            source_module: data.source_module,
        });
    }

    result
}

// ---- Internal helpers for parameter usage analysis ----

fn mark_used_params(ty: Ty, ctx: &dyn TypeLookup, used: &mut [bool]) {
    match ctx.ty_kind(ty) {
        TyKind::Param(ParamTy { index, .. }) => {
            let i = *index as usize;
            if i < used.len() {
                used[i] = true;
            }
        }
        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => {
            mark_used_params(*inner, ctx, used);
        }
        TyKind::Slice(inner) => {
            mark_used_params(*inner, ctx, used);
        }
        TyKind::Array(inner, len) => {
            mark_used_params(*inner, ctx, used);
            // Phase 2 (GLYIM_DESTUB_PLAN): also mark the length const's params
            // used, so polymorphize does not merge monomorphizations that differ
            // only in the array length (which would corrupt array layouts).
            mark_used_params_in_const(len, ctx, used);
        }
        TyKind::Tuple(substs)
        | TyKind::Adt(_, substs)
        | TyKind::Closure(_, substs)
        | TyKind::Opaque(_, substs) => {
            mark_used_params_in_subst(*substs, ctx, used);
        }
        TyKind::FnDef(_, substs) => {
            mark_used_params_in_subst(*substs, ctx, used);
        }
        TyKind::FnPtr(sig) => {
            for arg in ctx.substitution_args(sig.inputs) {
                if let GenericArg::Ty(t) = arg {
                    mark_used_params(*t, ctx, used);
                }
            }
            mark_used_params(sig.output, ctx, used);
        }
        TyKind::Dynamic(binder, _) => {
            for pred in binder.clone().skip_binder().iter() {
                mark_used_params_in_predicate(pred, ctx, used);
            }
        }
        TyKind::Projection(proj) => {
            mark_used_params_in_subst(proj.trait_ref.substs, ctx, used);
        }
        TyKind::Bound(_, _) | TyKind::Infer(_) | TyKind::Error => {
            // These shouldn't appear in MIR after typeck, but handle gracefully.
        }
        _ => {}
    }
}

fn mark_used_params_in_subst(substs: Substitution, ctx: &dyn TypeLookup, used: &mut [bool]) {
    for arg in ctx.substitution_args(substs) {
        match arg {
            GenericArg::Ty(t) => mark_used_params(*t, ctx, used),
            GenericArg::Lifetime(_) => {}
            GenericArg::Const(c) => mark_used_params_in_const(c, ctx, used),
        }
    }
}

fn mark_used_params_in_const(c: &Const, ctx: &dyn TypeLookup, used: &mut [bool]) {
    if let ConstKind::Param(ParamConst { index, .. }) = &c.kind {
        let i = *index as usize;
        if i < used.len() {
            used[i] = true;
        }
    }
    mark_used_params(c.ty, ctx, used);
}

fn mark_used_params_in_predicate(pred: &Predicate, ctx: &dyn TypeLookup, used: &mut [bool]) {
    match pred {
        Predicate::Trait(tp) => {
            mark_used_params_in_subst(tp.trait_ref.substs, ctx, used);
        }
        Predicate::TypeOutlives(top) => {
            mark_used_params(top.ty, ctx, used);
        }
        Predicate::RegionOutlives(_) => {}
        Predicate::WellFormed(ty) => {
            mark_used_params(*ty, ctx, used);
        }
        Predicate::Coerce(a, b) => {
            mark_used_params(*a, ctx, used);
            mark_used_params(*b, ctx, used);
        }
    }
}

fn mark_used_params_in_mir_const(c: &glyim_mir::MirConst, ctx: &dyn TypeLookup, used: &mut [bool]) {
    mark_used_params(c.ty, ctx, used);
    match &c.kind {
        MirConstKind::Fn(_, substs) | MirConstKind::ConstRef(_, substs) => {
            mark_used_params_in_subst(*substs, ctx, used);
        }
        _ => {}
    }
}

fn mark_used_params_in_operand(
    op: &Operand,
    local_decls: &IndexVec<LocalIdx, LocalDecl>,
    ctx: &dyn TypeLookup,
    used: &mut [bool],
) {
    match op {
        Operand::Constant(c) => mark_used_params_in_mir_const(c, ctx, used),
        // Copy/Move of a place: the place's *projection* may reference a generic
        // param (e.g. indexing `[T; N]` with a generic index local, or
        // field-projecting an ADT whose field types mention T). §8.11: recurse
        // into the projection so these uses are recorded.
        Operand::Copy(place) | Operand::Move(place) => {
            mark_used_params_in_place(place, local_decls, ctx, used);
        }
    }
}

/// Walk a place's projection chain, recording any generic parameters that the
/// projections themselves reference.
///
/// - `Index(local)` is a *use* of that local's type parameters if generic.
/// - `Field`/`ConstantIndex`/`Subslice` project through an ADT/tuple/array/slice
///   whose underlying type may mention parameters not otherwise visible. We
///   recover the projected type via [`glyim_mir::Place::ty`] and mark it, which
///   catches field types that carry the parameter.
fn mark_used_params_in_place(
    place: &glyim_mir::Place,
    local_decls: &IndexVec<LocalIdx, LocalDecl>,
    ctx: &dyn TypeLookup,
    used: &mut [bool],
) {
    // The base local's type is covered by the `for local in body.locals` pass,
    // but the *projection* can surface a parameter that only appears inside a
    // field/element type. Compute the projected type and mark it.
    let ty = place.ty(ctx, local_decls);
    mark_used_params(ty, ctx, used);

    // `Index(local)`: the index operand is itself a use of that local's type
    // parameters when generic; ensure the index local's type is marked.
    for elem in place.projection.iter() {
        if let ProjectionElem::Index(local) = elem
            && let Some(decl) = local_decls.get(*local)
        {
            mark_used_params(decl.ty, ctx, used);
        }
    }
}

fn mark_used_params_in_rvalue(
    rv: &Rvalue,
    local_decls: &IndexVec<LocalIdx, LocalDecl>,
    ctx: &dyn TypeLookup,
    used: &mut [bool],
) {
    match rv {
        Rvalue::Use(op) => mark_used_params_in_operand(op, local_decls, ctx, used),
        Rvalue::BinaryOp(_, boxed) => {
            let (l, r) = boxed.as_ref();
            mark_used_params_in_operand(l, local_decls, ctx, used);
            mark_used_params_in_operand(r, local_decls, ctx, used);
        }
        Rvalue::UnaryOp(_, op) => mark_used_params_in_operand(op, local_decls, ctx, used),
        Rvalue::Aggregate(kind, ops) => {
            match kind {
                AggregateKind::Array(ty) => mark_used_params(*ty, ctx, used),
                AggregateKind::Adt(_, _, substs) => mark_used_params_in_subst(*substs, ctx, used),
                AggregateKind::Closure(_, substs) => mark_used_params_in_subst(*substs, ctx, used),
                AggregateKind::Tuple => {}
            }
            for op in ops {
                mark_used_params_in_operand(op, local_decls, ctx, used);
            }
        }
        Rvalue::Cast(_, op, ty) => {
            mark_used_params_in_operand(op, local_decls, ctx, used);
            mark_used_params(*ty, ctx, used);
        }
        Rvalue::Repeat(op, mir_const) => {
            mark_used_params_in_operand(op, local_decls, ctx, used);
            mark_used_params_in_mir_const(mir_const, ctx, used);
        }
        // §8.11: Ref/Deref-of-place, Discriminant(place), Len(place) — the base
        // place (and its projection) can reference a generic parameter.
        Rvalue::Ref(place, _) => mark_used_params_in_place(place, local_decls, ctx, used),
        Rvalue::Discriminant(place) => mark_used_params_in_place(place, local_decls, ctx, used),
        Rvalue::Len(place) => mark_used_params_in_place(place, local_decls, ctx, used),
    }
}

fn mark_used_params_in_terminator(
    kind: &TerminatorKind,
    local_decls: &IndexVec<LocalIdx, LocalDecl>,
    ctx: &dyn TypeLookup,
    used: &mut [bool],
) {
    match kind {
        TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } => {
            mark_used_params_in_operand(func, local_decls, ctx, used);
            for arg in args {
                mark_used_params_in_operand(arg, local_decls, ctx, used);
            }
            // §8.11: the call's destination place (and its projection) may carry
            // a parameter (e.g. storing into a field of a generic ADT).
            mark_used_params_in_place(destination, local_decls, ctx, used);
        }
        TerminatorKind::SwitchInt {
            discr, switch_ty, ..
        } => {
            mark_used_params_in_operand(discr, local_decls, ctx, used);
            mark_used_params(*switch_ty, ctx, used);
        }
        TerminatorKind::Assert { cond, .. } => {
            mark_used_params_in_operand(cond, local_decls, ctx, used);
        }
        // §8.11: Drop(place) — the dropped place (and its projection) can
        // reference a generic parameter (e.g. dropping a field of a generic ADT).
        TerminatorKind::Drop { place, .. } => {
            mark_used_params_in_place(place, local_decls, ctx, used);
        }
        TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_core::interner::Interner;
    use glyim_core::primitives::{IntTy, UintTy};
    use glyim_type::const_val::{Const, ConstKind, ParamConst};
    use glyim_type::ty_ctx_mut::TyCtxMut;

    /// Phase 2 (GLYIM_DESTUB_PLAN), Step 2b: `mark_used_params` must mark the
    /// array length const's params as used. A `[i32; N]` with `N = ParamConst(0)`
    /// must set `used[0] = true`, so polymorphize does not merge
    /// `[i32; 3]` and `[i32; 7]` into one mono item (which would corrupt the
    /// array layout).
    #[test]
    fn mark_used_params_marks_array_length_const() {
        let mut tcx = TyCtxMut::new(Interner::new());
        let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
        let usize_ty = tcx.mk_ty(TyKind::Uint(UintTy::Usize));
        let len = Const {
            kind: ConstKind::Param(ParamConst {
                index: 0,
                name: Interner::new().intern("N"),
            }),
            ty: usize_ty,
        };
        let arr = tcx.mk_ty(TyKind::Array(i32_ty, len));
        let frozen = tcx.freeze();

        let mut used = vec![false];
        mark_used_params(arr, &frozen, &mut used);

        assert!(
            used[0],
            "array length const ParamConst(0) must be marked used"
        );
    }
}
