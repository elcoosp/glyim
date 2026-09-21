//! Mock implementation of LowerCtx for testing.
use glyim_core::def_id::{AdtId, ConstDefId, FnDefId};
use glyim_core::interner::Name;
use glyim_lower::{AdtDef, AdtKind, IteratorNextInfo, LowerCtx};
use glyim_mir;
use glyim_span::Span;
use glyim_type::{FieldIdx, FnSig, Substitution, Ty, TyCtx};
use std::cell::RefCell;
use std::collections::HashMap;

/// Key for field index lookups: (AdtId raw, variant_idx, Name)
type FieldKey = (u32, u32, Name);

/// Key for variant index lookups: (AdtId raw, Name)
type VariantKey = (u32, Name);

/// MockLowerCtx.
pub struct MockLowerCtx<'a> {
    ty_ctx: &'a TyCtx,
    span_stack: RefCell<Vec<Span>>,
    /// Map from (AdtId raw, variant_idx, field_name) to field index
    field_indices: HashMap<FieldKey, FieldIdx>,
    /// Map from (AdtId raw, variant_name) to variant index
    variant_indices: HashMap<VariantKey, u32>,
    /// ADT definitions keyed by AdtId raw
    adt_defs: HashMap<u32, AdtDef>,
    /// Optional override for `iterator_next_fn` so tests can simulate
    /// "solver resolved Iterator::next" vs "solver didn't" in isolation.
    iterator_next_override: Option<Box<dyn Fn(Ty, Ty) -> Option<IteratorNextInfo> + 'a>>,
}

/// Operations for span testing.
pub enum SpanOp {
    #[allow(missing_docs)]
    Push(Span),
    /// Variant.
    Pop,
}

impl<'a> MockLowerCtx<'a> {
    /// new.
    pub fn new(ty_ctx: &'a TyCtx) -> Self {
        Self {
            ty_ctx,
            span_stack: RefCell::new(Vec::new()),
            field_indices: HashMap::new(),
            variant_indices: HashMap::new(),
            adt_defs: HashMap::new(),
            iterator_next_override: None,
        }
    }

    /// Register a field index for an ADT variant.
    pub fn add_field_index(
        &mut self,
        adt_id: AdtId,
        variant_idx: u32,
        field_name: Name,
        field_idx: FieldIdx,
    ) {
        self.field_indices
            .insert((adt_id.to_raw(), variant_idx, field_name), field_idx);
    }

    /// Register a variant index for an ADT.
    pub fn add_variant_index(&mut self, adt_id: AdtId, variant_name: Name, variant_idx: u32) {
        self.variant_indices
            .insert((adt_id.to_raw(), variant_name), variant_idx);
    }

    /// Register an ADT definition.
    pub fn add_adt_def(&mut self, adt_id: AdtId, def: AdtDef) {
        self.adt_defs.insert(adt_id.to_raw(), def);
    }

    /// Convenience: attach an iterator‑next resolver. When set, this closure is
    /// consulted by the `LowerCtx::iterator_next_fn` implementation, letting a
    /// test simulate "Iterator::next resolved" (return `Some(info)`) versus
    /// "solver didn't find it" (return `None`) without a full pipeline.
    pub fn with_iterator_next<F>(mut self, f: F) -> Self
    where
        F: Fn(glyim_type::Ty, glyim_type::Ty) -> Option<glyim_lower::IteratorNextInfo> + 'a,
    {
        self.iterator_next_override = Some(Box::new(f));
        self
    }
}

impl<'a> LowerCtx for MockLowerCtx<'a> {
    fn ty_ctx(&self) -> &TyCtx {
        self.ty_ctx
    }

    fn adt_def(&self, id: AdtId) -> AdtDef {
        self.adt_defs.get(&id.to_raw()).cloned().unwrap_or(AdtDef {
            variants: vec![],
            kind: AdtKind::Struct,
        })
    }

    fn hir_body(&self, _owner: glyim_core::def_id::LocalDefId) -> Option<&glyim_hir::Body> {
        // Mock context does not have a real HIR map, so return None.
        None
    }

    fn push_span(&self, span: Span) {
        self.span_stack.borrow_mut().push(span);
    }

    fn pop_span(&self) {
        self.span_stack.borrow_mut().pop();
    }

    fn field_index_by_name(&self, adt_id: AdtId, variant_idx: u32, name: Name) -> Option<FieldIdx> {
        self.field_indices
            .get(&(adt_id.to_raw(), variant_idx, name))
            .copied()
    }

    fn variant_index_by_name(&self, adt_id: AdtId, name: Name) -> Option<u32> {
        self.variant_indices.get(&(adt_id.to_raw(), name)).copied()
    }

    fn fn_sig(&self, _def_id: FnDefId) -> Option<FnSig> {
        // Provide a minimal dummy signature for tests.
        Some(FnSig {
            inputs: Substitution::empty(),
            output: self.ty_ctx.unit_ty(),
            c_variadic: false,
            unsafety: glyim_core::primitives::Safety::Safe,
            abi: glyim_core::primitives::Abi::Glyim,
        })
    }

    fn const_value(
        &self,
        _def_id: ConstDefId,
        _substs: Substitution,
    ) -> Option<glyim_mir::MirConst> {
        Some(glyim_mir::MirConst {
            kind: glyim_mir::MirConstKind::Unit,
            ty: self.ty_ctx.unit_ty(),
            span: Span::DUMMY,
        })
    }

    fn iterator_next_fn(&self, iter_ty: Ty, elem_ty: Ty) -> Option<IteratorNextInfo> {
        self.iterator_next_override
            .as_ref()
            .and_then(|f| f(iter_ty, elem_ty))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_frozen_ty_ctx;
    use glyim_core::def_id::FnDefId;

    fn sample_info() -> IteratorNextInfo {
        IteratorNextInfo {
            fn_def_id: FnDefId::from_raw(0),
            fn_substs: Substitution::empty(),
            fn_ty: Ty::ERROR,
            option_ty: Ty::UNIT,
            discr_ty: Ty::UNIT,
            ref_iter_ty: Ty::ERROR,
        }
    }

    #[test]
    fn test_iterator_next_override_resolved() {
        // Tier 7.3: with_iterator_next wires the closure into iterator_next_fn,
        // so a test can simulate "solver resolved Iterator::next".
        let ctx = test_frozen_ty_ctx();
        let info = sample_info();
        let mock =
            MockLowerCtx::new(&ctx).with_iterator_next(move |_iter, _elem| Some(info.clone()));
        let got = LowerCtx::iterator_next_fn(&mock, Ty::UNIT, Ty::UNIT);
        assert!(
            got.is_some(),
            "iterator_next_fn should return the override's Some(info)"
        );
        assert_eq!(got.unwrap().fn_def_id, FnDefId::from_raw(0));
    }

    #[test]
    fn test_iterator_next_no_override_is_none() {
        // Without an override the default trait behavior (None) is preserved.
        let ctx = test_frozen_ty_ctx();
        let mock = MockLowerCtx::new(&ctx);
        assert!(LowerCtx::iterator_next_fn(&mock, Ty::UNIT, Ty::UNIT).is_none());
    }

    #[test]
    fn test_iterator_next_override_can_return_none() {
        // The closure can also simulate "solver didn't find next".
        let ctx = test_frozen_ty_ctx();
        let mock = MockLowerCtx::new(&ctx).with_iterator_next(|_iter, _elem| None);
        assert!(LowerCtx::iterator_next_fn(&mock, Ty::UNIT, Ty::UNIT).is_none());
    }
}
