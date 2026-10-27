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
    /// Explicitly-registered function signatures. `LowerCtx::fn_sig` is a
    /// *registry query*, not a "guaranteed answer" — the production ctx
    /// pulls real signatures from typeck; the mock has no such source. The
    /// audit (`P2-A2`) flagged that the old implementation returned a
    /// dummy `() -> ()` for **every** `FnDefId`, silently answering "yes,
    /// that signature exists" to any caller. A test exercising the call
    /// lowering path would then run against an ABI that would never exist
    /// in a real compile — a plausible-looking but wrong test.
    ///
    /// Post-fix contract: `fn_sig` returns `Some` only for ids explicitly
    /// registered here, and `panic!`s otherwise. A test that needs a
    /// signature must be explicit about which one it needs.
    registered_fn_sigs: HashMap<FnDefId, FnSig>,
    /// Explicitly-registered const values. Same reasoning as
    /// `registered_fn_sigs`: the old arm returned `Some(Unit)` for any
    /// `ConstDefId`, which made any const-eval lowering test pass against
    /// a value that would never appear in real compilation.
    registered_const_values: HashMap<ConstDefId, glyim_mir::MirConst>,
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
            registered_fn_sigs: HashMap::new(),
            registered_const_values: HashMap::new(),
            iterator_next_override: None,
        }
    }

    /// Register a `FnDefId -> FnSig` mapping. `fn_sig` returns `Some` for
    /// this id and panics for any other.
    pub fn add_fn_sig(&mut self, def_id: FnDefId, sig: FnSig) {
        self.registered_fn_sigs.insert(def_id, sig);
    }

    /// Register a `ConstDefId -> MirConst` mapping. `const_value` returns
    /// `Some` for this id and panics for any other.
    pub fn add_const_value(&mut self, def_id: ConstDefId, value: glyim_mir::MirConst) {
        self.registered_const_values.insert(def_id, value);
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

    fn fn_sig(&self, def_id: FnDefId) -> Option<FnSig> {
        // P2-A2: panic-until-registered. The old implementation returned a
        // dummy `() -> ()` signature for **any** `FnDefId`, silently
        // answering "yes, that signature exists" to any caller. A test that
        // exercised call lowering then ran against an ABI that would never
        // appear in a real compile. Post-fix: an id must be registered via
        // `add_fn_sig` first, or this panics with the id, so a test cannot
        // accidentally drift onto a plausible-but-wrong signature.
        Some(self.registered_fn_sigs.get(&def_id).cloned().unwrap_or_else(|| {
            panic!(
                "MockLowerCtx::fn_sig({:?}) called with no registered signature. \
                 Call `add_fn_sig(def_id, sig)` in the test setup first \
                 (P2-A2: mocks must not answer with dummies).",
                def_id
            )
        }))
    }

    fn const_value(
        &self,
        def_id: ConstDefId,
        _substs: Substitution,
    ) -> Option<glyim_mir::MirConst> {
        // P2-A2: same panic-until-registered contract as `fn_sig` above.
        Some(
            self.registered_const_values
                .get(&def_id)
                .cloned()
                .unwrap_or_else(|| {
                    panic!(
                        "MockLowerCtx::const_value({:?}) called with no registered value. \
                         Call `add_const_value(def_id, value)` in the test setup first \
                         (P2-A2: mocks must not answer with dummies).",
                        def_id
                    )
                }),
        )
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

// P2-A2 regression tests: `fn_sig` / `const_value` must panic when the id
// is unregistered, and return the registered value when it is. These are
// separate `mod` entries so a future test that wants to construct a
// signature must explicitly register it (the `add_fn_sig` / `add_const_value`
// calls below are the pattern).
#[cfg(test)]
mod p2_a2_tests {
    use super::*;
    use crate::test_frozen_ty_ctx;
    use glyim_core::def_id::FnDefId;
    use glyim_mir::{MirConst, MirConstKind};
    use glyim_span::Span;
    use glyim_type::{FnSig, Substitution};

    #[test]
    fn fn_sig_returns_registered_signature() {
        let ctx = test_frozen_ty_ctx();
        let mut mock = MockLowerCtx::new(&ctx);
        let dummy_id = FnDefId::from_raw(4242);
        let sig = FnSig {
            inputs: Substitution::empty(),
            output: ctx.unit_ty(),
            c_variadic: false,
            unsafety: glyim_core::primitives::Safety::Safe,
            abi: glyim_core::primitives::Abi::Glyim,
        };
        mock.add_fn_sig(dummy_id, sig.clone());
        let got = mock.fn_sig(dummy_id).expect("registered sig");
        assert_eq!(got.output, sig.output);
        assert_eq!(got.c_variadic, sig.c_variadic);
    }

    #[test]
    #[should_panic(expected = "MockLowerCtx::fn_sig")]
    fn fn_sig_panics_when_unregistered() {
        let ctx = test_frozen_ty_ctx();
        let mock = MockLowerCtx::new(&ctx);
        let _ = mock.fn_sig(FnDefId::from_raw(9999));
    }

    #[test]
    fn const_value_returns_registered_value() {
        let ctx = test_frozen_ty_ctx();
        let mut mock = MockLowerCtx::new(&ctx);
        let dummy_id = glyim_core::def_id::ConstDefId::from_raw(1);
        let val = MirConst {
            kind: MirConstKind::Unit,
            ty: ctx.unit_ty(),
            span: Span::DUMMY,
        };
        mock.add_const_value(dummy_id, val);
        let got = mock.const_value(dummy_id, Substitution::empty()).expect("registered const");
        assert_eq!(got.ty, ctx.unit_ty());
    }

    #[test]
    #[should_panic(expected = "MockLowerCtx::const_value")]
    fn const_value_panics_when_unregistered() {
        let ctx = test_frozen_ty_ctx();
        let mock = MockLowerCtx::new(&ctx);
        let _ = mock.const_value(
            glyim_core::def_id::ConstDefId::from_raw(7),
            Substitution::empty(),
        );
    }
}
