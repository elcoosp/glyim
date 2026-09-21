use crate::GenericArg;
use crate::ty::Ty;
use glyim_core::def_id::AdtId;
use std::collections::HashMap;

/// Phase 5 (GLYIM_DESTUB_PLAN): registry mapping a `Deref`-implementing type
/// to its `Deref::Target`, so `TyCtx::deref_ty` can perform autoderef for user
/// `impl Deref` items (not just `&T`/`*T`).
///
/// Two views are kept:
/// * `targets` — exact `SelfTy -> Target` (fast path for concrete queries and
///   non-ADT self types).
/// * `templates` — per `AdtId`, `(self_params, target)` for generic
///   `impl<T> Deref for Adt<T> { type Target = T; }`. At query time the
///   concrete arguments substitute into `target` (see `TyCtx::deref_ty`).
#[derive(Debug, Clone, Default)]
pub struct DerefRegistry {
    pub(crate) targets: HashMap<Ty, Ty>,
    pub(crate) templates: HashMap<AdtId, (Vec<GenericArg>, Ty)>,
}

impl DerefRegistry {
    pub fn new() -> Self {
        DerefRegistry {
            targets: HashMap::new(),
            templates: HashMap::new(),
        }
    }

    /// Register an exact `SelfTy -> Target` mapping (always recorded).
    pub fn register_deref_impl(&mut self, self_ty: Ty, target_ty: Ty) {
        self.targets.insert(self_ty, target_ty);
    }

    /// Register a generic template keyed by the self ADT id, used when `self_ty`
    /// is `Adt<T0, T1, …>` and `Target` references those parameters.
    pub fn register_deref_template(
        &mut self,
        adt_id: AdtId,
        self_params: Vec<GenericArg>,
        target_ty: Ty,
    ) {
        self.templates.insert(adt_id, (self_params, target_ty));
    }

    /// Exact `SelfTy -> Target` lookup.
    pub fn exact_target(&self, self_ty: Ty) -> Option<Ty> {
        self.targets.get(&self_ty).copied()
    }

    /// Template `(self_params, target)` for a given ADT id.
    pub fn template(&self, adt_id: AdtId) -> Option<&(Vec<GenericArg>, Ty)> {
        self.templates.get(&adt_id)
    }
}
