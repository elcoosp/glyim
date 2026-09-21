use glyim_borrowck::BorrowckCtx;
use glyim_mir::{Body, LocalDecl, LocalIdx};
use glyim_type::{Ty, TyCtx};

/// MockBorrowckCtx.
pub struct MockBorrowckCtx<'a> {
    /// Struct.
    pub ty_ctx: &'a TyCtx,
    /// Struct.
    pub body: &'a Body,
}

impl<'a> MockBorrowckCtx<'a> {
    /// new.
    pub fn new(ty_ctx: &'a TyCtx, body: &'a Body) -> Self {
        Self { ty_ctx, body }
    }
}

impl BorrowckCtx for MockBorrowckCtx<'_> {
    fn ty_ctx(&self) -> &TyCtx {
        self.ty_ctx
    }
    fn local_decl(&self, local: LocalIdx) -> &LocalDecl {
        &self.body.locals[local]
    }
    fn is_copy(&self, _ty: Ty) -> bool {
        false
    }
    fn local_name(&self, idx: LocalIdx) -> String {
        format!("_{}", idx.to_raw())
    }
}
