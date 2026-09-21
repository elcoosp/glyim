//! Phase 5 (GLYIM_DESTUB_PLAN): population of `TyCtx::deref_registry` from
//! real `impl Deref for X { type Target = Y; }` items.
//!
//! `resolve_method_call` already loops `TyCtx::deref_ty` at each autoderef
//! step (check_expr.rs ~L1250). For ADT receivers (`Box<T>`, `Rc<T>`, …) the
//! structural `&T`/`*T` arms do nothing, so unless the Deref registry is
//! populated, a method defined only on the inner type (`Vec::push` reached via
//! `Box<Vec<i32>>`) can never be found. This module builds the registry during
//! typeck's HIR scan (mirroring `auto_trait_registry` population) so that
//! `deref_ty` can step through user `Deref` impls.
//!
//! Generic impls (`impl<T> Deref for Box<T> { type Target = T; }`) are handled
//! by `TyCtxMut::register_deref_impl`, which records a per-`AdtId` template.
//! At query time `deref_ty` substitutes the concrete argument into the target
//! parameter positionally (real `Deref` impls always use `type Target = T`).

use crate::tyconv::{build_param_tys, resolve_type_ref};
use glyim_core::interner::Name;
use glyim_hir::ItemKind;
use glyim_type::{Ty, TyCtxMut, TyKind};

use crate::{CrateDefMap, GlyimDiagnostic, InferenceTable};

/// Phase 5 (GLYIM_DESTUB_PLAN): walk every `impl Deref for SelfTy { type
/// Target = T; }` item and register `SelfTy -> Target` in the deref registry.
pub fn populate_deref_registry(
    ctx: &mut TyCtxMut,
    hir: &glyim_hir::CrateHir,
    def_map: &CrateDefMap,
    infer: &mut InferenceTable,
    diagnostics: &mut Vec<GlyimDiagnostic>,
) {
    let deref_name: Name = ctx.resolver().intern("Deref");
    let target_name: Name = ctx.resolver().intern("Target");

    for (_id, item) in hir.items.iter_enumerated() {
        let impl_item = match &item.kind {
            ItemKind::Impl(impl_item) => impl_item,
            _ => continue,
        };

        // Only `impl Deref for …` items.
        let trait_name = match impl_item.trait_ref.as_ref().and_then(|p| p.as_name()) {
            Some(n) if n == deref_name => n,
            _ => continue,
        };
        let _ = trait_name;

        let param_map = build_param_tys(ctx, &impl_item.generic_params);

        // Self type of the impl.
        let self_ty = resolve_type_ref(
            ctx,
            infer,
            def_map,
            diagnostics,
            &impl_item.self_ty,
            &param_map,
            item.span,
        );
        if matches!(ctx.ty_kind(self_ty), TyKind::Error) {
            continue;
        }

        // Find `type Target = …;` and resolve its type.
        let target_ty: Option<Ty> = impl_item
            .associated_types
            .iter()
            .find(|at| at.name == target_name)
            .and_then(|at| at.default.as_ref())
            .map(|ty_ref| {
                resolve_type_ref(
                    ctx,
                    infer,
                    def_map,
                    diagnostics,
                    ty_ref,
                    &param_map,
                    item.span,
                )
            });
        let target_ty = match target_ty {
            Some(t) if !matches!(ctx.ty_kind(t), TyKind::Error) => t,
            _ => continue,
        };

        // Register the resolved pair. For generic self types (`Box<T>`),
        // `register_deref_impl` additionally records a per-`AdtId` template so
        // a concrete `Box<Vec<i32>>` re-matches when autoderef steps through
        // it (the target is substituted positionally in `deref_ty`).
        ctx.register_deref_impl(self_ty, target_ty);
    }
}
