use glyim_diag::{DiagSeverity, GlyimDiagnostic, MultiSpan};
use glyim_mir::{MirConstKind, Operand, Rvalue, StatementKind, TerminatorKind};
use glyim_span::Span;
use glyim_type::{TyCtx, TyKind};

use crate::mono::MonoItem;
use crate::mono::MonoItemData;

/// Check for unsized locals in instantiated MIR bodies.
pub fn check_unsized_locals(items: &[MonoItemData], ctx: &TyCtx) -> Vec<GlyimDiagnostic> {
    let mut diags = Vec::new();
    for item in items {
        for local_decl in item.body.locals.iter() {
            let ty = local_decl.ty;
            let kind = ctx.ty_kind(ty);
            if matches!(kind, TyKind::Slice(_) | TyKind::Dynamic(_, _)) {
                let msg = format!(
                    "unsized local variable of type `{}`",
                    glyim_type::PrintTy::new(ty, ctx)
                );
                diags.push(GlyimDiagnostic::type_error(
                    local_decl.source_info.span,
                    msg,
                ));
            }
        }
    }
    diags
}

/// Warn if the number of mono items exceeds the given threshold.
pub fn check_large_mono_set(items: &[MonoItemData], threshold: usize) -> Vec<GlyimDiagnostic> {
    if items.len() > threshold {
        let msg = format!(
            "large number of mono items: {} (threshold: {}); consider reducing generic instantiations",
            items.len(),
            threshold
        );
        // No single source location owns a "the mono set is large" warning;
        // pointing at the first monomorphized item's span is the most useful
        // real location available (it names *a* function involved, which the
        // user can act on). Falls back to `Span::DUMMY` only when no item has
        // a usable span at all.
        let span = items
            .iter()
            .map(|it| it.body.span)
            .find(|s| !s.is_dummy())
            .unwrap_or(Span::DUMMY);
        vec![GlyimDiagnostic::new(
            glyim_diag::ErrorCode {
                category: glyim_diag::ErrorCategory::Internal,
                number: 0,
            },
            DiagSeverity::Warning,
            msg,
            MultiSpan::from_span(span),
        )]
    } else {
        vec![]
    }
}

/// Check for unused generic parameters in monomorphized items.
///
/// If a function has type parameters (substitution non-empty) but none of those
/// parameters appear in the body's types, a warning is emitted.
///
/// **The body must be the pre-substitution body.** `MonoItemData::body` is
/// post-substitution: `MonoCtx::collect` ran it through `substitute_body`,
/// which rewrites every `TyKind::Param(_)` to a concrete type. Checking that
/// body for `Param` always finds nothing and warns for *every* generic
/// function, however it used its parameters (e.g. `block_on<F: Future>` is
/// warned about even though `F` is used throughout its body).
///
/// `pre_mono_bodies` is the pre-substitution map (`mir_bodies_map` in the
/// pipeline), where `Param` references still appear. `None` (the unit tests'
/// path — their hand-built bodies already carry `Param` when relevant) falls
/// back to `item.body`.
pub fn check_unused_generic_params(
    items: &[MonoItemData],
    pre_mono_bodies: Option<
        &std::collections::HashMap<
            glyim_core::def_id::DefId,
            std::sync::Arc<glyim_mir::Body>,
        >,
    >,
    ctx: &TyCtx,
) -> Vec<GlyimDiagnostic> {
    let mut diags = Vec::new();
    for item in items {
        if let MonoItem::Fn { def_id, substs } = &item.item {
            if substs.is_empty() {
                continue;
            }
            let checked_body = pre_mono_bodies
                .and_then(|m| {
                    m.get(&glyim_core::def_id::DefId::new(
                        glyim_core::def_id::CrateId::from_raw(0),
                        glyim_core::def_id::LocalDefId::from_raw(def_id.to_raw()),
                    ))
                })
                .map(|arc| arc.as_ref())
                .unwrap_or_else(|| item.body.as_ref());
            if !body_uses_any_param(checked_body, ctx) {
                let msg = format!("unused generic parameter(s) in function `{}`", item.symbol);
                diags.push(GlyimDiagnostic::new(
                    glyim_diag::ErrorCode {
                        category: glyim_diag::ErrorCategory::Type,
                        number: 0,
                    },
                    DiagSeverity::Warning,
                    msg,
                    MultiSpan::from_span(item.body.span),
                ));
            }
        }
    }
    diags
}

fn body_uses_any_param(body: &glyim_mir::Body, ctx: &TyCtx) -> bool {
    // Check locals
    for local in body.locals.iter() {
        if ty_contains_param(local.ty, ctx) {
            return true;
        }
    }

    // Check statements and terminators
    for block in body.basic_blocks.iter() {
        for stmt in &block.statements {
            match &stmt.kind {
                StatementKind::Assign(_place, rvalue) => {
                    if rvalue_contains_param(rvalue, ctx) {
                        return true;
                    }
                }
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Nop => {}
            }
        }
        match &block.terminator.kind {
            TerminatorKind::Call { func, args, .. } => {
                if operand_contains_param(func, ctx) {
                    return true;
                }
                for arg in args {
                    if operand_contains_param(arg, ctx) {
                        return true;
                    }
                }
            }
            TerminatorKind::SwitchInt {
                discr, switch_ty, ..
            } => {
                if operand_contains_param(discr, ctx) {
                    return true;
                }
                if ty_contains_param(*switch_ty, ctx) {
                    return true;
                }
            }
            TerminatorKind::Assert { cond, .. } => {
                if operand_contains_param(cond, ctx) {
                    return true;
                }
            }
            TerminatorKind::Drop { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => {}
        }
    }
    false
}

fn operand_contains_param(op: &Operand, ctx: &TyCtx) -> bool {
    match op {
        Operand::Copy(_) | Operand::Move(_) => false,
        Operand::Constant(mir_const) => {
            if ty_contains_param(mir_const.ty, ctx) {
                return true;
            }
            match &mir_const.kind {
                MirConstKind::Fn(_, substs) | MirConstKind::ConstRef(_, substs) => {
                    subst_args_contain_param(*substs, ctx)
                }
                _ => false,
            }
        }
    }
}

fn rvalue_contains_param(rv: &Rvalue, ctx: &TyCtx) -> bool {
    match rv {
        Rvalue::Use(op) => operand_contains_param(op, ctx),
        Rvalue::BinaryOp(_, boxed) => {
            let (lhs, rhs) = boxed.as_ref();
            operand_contains_param(lhs, ctx) || operand_contains_param(rhs, ctx)
        }
        Rvalue::UnaryOp(_, op) => operand_contains_param(op, ctx),
        Rvalue::Ref(_, _) => false,
        Rvalue::Aggregate(kind, ops) => {
            if let glyim_mir::AggregateKind::Array(ty) = kind
                && ty_contains_param(*ty, ctx)
            {
                return true;
            }
            if let glyim_mir::AggregateKind::Adt(_, _, substs) = kind
                && subst_args_contain_param(*substs, ctx)
            {
                return true;
            }
            if let glyim_mir::AggregateKind::Closure(_, substs) = kind
                && subst_args_contain_param(*substs, ctx)
            {
                return true;
            }
            for op in ops {
                if operand_contains_param(op, ctx) {
                    return true;
                }
            }
            false
        }
        Rvalue::Discriminant(_) | Rvalue::Len(_) => false,
        Rvalue::Cast(_, op, ty) => operand_contains_param(op, ctx) || ty_contains_param(*ty, ctx),
        Rvalue::Repeat(op, _) => operand_contains_param(op, ctx),
    }
}

fn ty_contains_param(ty: glyim_type::Ty, ctx: &TyCtx) -> bool {
    let kind = ctx.ty_kind(ty);
    match kind {
        TyKind::Param(_) => true,
        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => ty_contains_param(*inner, ctx),
        TyKind::Slice(inner) => ty_contains_param(*inner, ctx),
        TyKind::Array(inner, _) => ty_contains_param(*inner, ctx),
        TyKind::Tuple(substs)
        | TyKind::Adt(_, substs)
        | TyKind::Closure(_, substs)
        | TyKind::Opaque(_, substs) => subst_args_contain_param(*substs, ctx),
        TyKind::FnDef(_, substs) => subst_args_contain_param(*substs, ctx),
        TyKind::FnPtr(sig) => {
            for input_ty in ctx.substitution_args(sig.inputs) {
                if let glyim_type::GenericArg::Ty(ty) = input_ty
                    && ty_contains_param(*ty, ctx)
                {
                    return true;
                }
            }
            ty_contains_param(sig.output, ctx)
        }
        TyKind::Dynamic(_binder, _) => {
            // binder contains predicates; simplified check
            false
        }
        TyKind::Projection(_) => false,
        _ => false,
    }
}

fn subst_args_contain_param(substs: glyim_type::Substitution, ctx: &TyCtx) -> bool {
    for arg in ctx.substitution_args(substs) {
        if let glyim_type::GenericArg::Ty(ty) = arg
            && ty_contains_param(*ty, ctx)
        {
            return true;
        }
    }
    false
}
