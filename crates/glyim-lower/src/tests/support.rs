use crate::lower::{AdtDef, IteratorNextInfo, LowerCtx};
use glyim_span::Span;
use glyim_type::*;

pub struct MockLowerCtx<'a> {
    pub ty_ctx: &'a TyCtx,
    pub iterator_next_fn: Option<Box<dyn Fn(Ty, Ty) -> Option<IteratorNextInfo> + 'a>>,
}

impl<'a> MockLowerCtx<'a> {
    pub fn new(ctx: &'a TyCtx) -> Self {
        Self {
            ty_ctx: ctx,
            iterator_next_fn: None,
        }
    }

    pub fn with_iterator_next<F>(mut self, f: F) -> Self
    where
        F: Fn(Ty, Ty) -> Option<IteratorNextInfo> + 'a,
    {
        self.iterator_next_fn = Some(Box::new(f));
        self
    }
}

impl LowerCtx for MockLowerCtx<'_> {
    fn ty_ctx(&self) -> &TyCtx {
        self.ty_ctx
    }

    fn adt_def(&self, _id: glyim_core::def_id::AdtId) -> AdtDef {
        AdtDef {
            variants: vec![],
            kind: crate::lower::AdtKind::Struct,
        }
    }
    fn hir_body(&self, _owner: glyim_core::def_id::LocalDefId) -> Option<&glyim_hir::Body> {
        None
    }
    fn push_span(&self, _span: Span) {}
    fn pop_span(&self) {}

    fn iterator_next_fn(&self, iter_ty: Ty, elem_ty: Ty) -> Option<IteratorNextInfo> {
        self.iterator_next_fn
            .as_ref()
            .and_then(|f| f(iter_ty, elem_ty))
    }
}
