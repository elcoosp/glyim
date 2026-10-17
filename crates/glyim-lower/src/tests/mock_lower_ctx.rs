use crate::lower::{AdtDef, AdtKind, AdtVariant, IteratorNextInfo, LowerCtx};
use glyim_core::def_id::{AdtId, ConstDefId, FnDefId};
use glyim_core::interner::Name;
use glyim_span::Span;
use glyim_type::{FieldIdx, FnSig, Substitution, Ty, TyCtx};
use std::collections::HashMap;

type FieldKey = (u32, u32, Name);

pub struct TestLowerCtx<'a> {
    pub ty_ctx: &'a TyCtx,
    field_indices: HashMap<FieldKey, FieldIdx>,
    /// Pre-constructed iterator info for for-loop lowering tests.
    iterator_next_info: Option<IteratorNextInfo>,
}

impl<'a> TestLowerCtx<'a> {
    pub fn new(ty_ctx: &'a TyCtx) -> Self {
        Self {
            ty_ctx,
            field_indices: HashMap::new(),
            iterator_next_info: None,
        }
    }

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

    /// Set the `IteratorNextInfo` returned by `iterator_next_fn`.
    pub fn set_iterator_next_info(&mut self, info: IteratorNextInfo) {
        self.iterator_next_info = Some(info);
    }
}

impl<'a> LowerCtx for TestLowerCtx<'a> {
    fn ty_ctx(&self) -> &TyCtx {
        self.ty_ctx
    }

    fn adt_def(&self, _id: AdtId) -> AdtDef {
        AdtDef {
            variants: vec![AdtVariant::new(vec![])],
            kind: AdtKind::Struct,
        }
    }
    fn hir_body(&self, _owner: glyim_core::def_id::LocalDefId) -> Option<&glyim_hir::Body> {
        None
    }
    fn push_span(&self, _span: Span) {}
    fn pop_span(&self) {}

    fn field_index_by_name(&self, adt_id: AdtId, variant_idx: u32, name: Name) -> Option<FieldIdx> {
        self.field_indices
            .get(&(adt_id.to_raw(), variant_idx, name))
            .copied()
    }

    fn variant_index_by_name(&self, _adt_id: AdtId, _name: Name) -> Option<u32> {
        None
    }

    fn fn_sig(&self, _def_id: FnDefId) -> Option<FnSig> {
        None
    }

    fn const_value(
        &self,
        _def_id: ConstDefId,
        _substs: Substitution,
    ) -> Option<glyim_mir::MirConst> {
        None
    }

    fn iterator_next_fn(&self, _iter_ty: Ty, _elem_ty: Ty) -> Option<IteratorNextInfo> {
        self.iterator_next_info.clone()
    }
}
