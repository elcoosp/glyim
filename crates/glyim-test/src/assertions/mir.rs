use glyim_mir::*;
use glyim_type::TyCtx;

/// assert_mir.
pub fn assert_mir<'a>(ctx: &'a TyCtx, body: &'a Body) -> MirAssert<'a> {
    MirAssert { ctx, body }
}

/// MirAssert.
pub struct MirAssert<'a> {
    ctx: &'a TyCtx,
    body: &'a Body,
}

impl MirAssert<'_> {
    /// block_count.
    pub fn block_count(self, expected: usize) -> Self {
        assert_eq!(self.body.basic_blocks.len(), expected);
        self
    }
    /// local_count.
    pub fn local_count(self, expected: usize) -> Self {
        assert_eq!(self.body.locals.len(), expected);
        self
    }
    /// block_terminator.
    pub fn block_terminator(self, block: BasicBlockIdx, expected: &str) -> Self {
        let actual = match &self.body.basic_blocks[block].terminator.kind {
            TerminatorKind::Goto { .. } => "Goto",
            TerminatorKind::Return => "Return",
            TerminatorKind::Unreachable => "Unreachable",
            TerminatorKind::Call { .. } => "Call",
            TerminatorKind::Drop { .. } => "Drop",
            TerminatorKind::SwitchInt { .. } => "SwitchInt",
            TerminatorKind::Assert { .. } => "Assert",
        };
        assert_eq!(actual, expected, "bb{} terminator", block.to_raw());
        self
    }
    /// local_ty.
    pub fn local_ty(self, local: LocalIdx, expected: &glyim_type::TyKind) -> Self {
        let actual = self.ctx.ty_kind(self.body.locals[local].ty);
        assert_eq!(actual, expected, "local {} ty", local.to_raw());
        self
    }
}
