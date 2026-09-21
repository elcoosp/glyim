/// MirGenTester.
pub struct MirGenTester;

impl MirGenTester {
    /// lower_body.
    pub fn lower_body(
        ctx: &mut dyn glyim_lower::LowerCtx,
        thir: &glyim_typeck::thir::Body,
    ) -> Result<glyim_mir::Body, Vec<glyim_diag::GlyimDiagnostic>> {
        let result = glyim_lower::lower_body(ctx, thir);
        if result.diagnostics.is_empty() {
            Ok(result.body)
        } else {
            Err(result.diagnostics)
        }
    }
    /// check_borrows.
    pub fn check_borrows(
        ctx: &dyn glyim_borrowck::BorrowckCtx,
        body: &glyim_mir::Body,
    ) -> glyim_borrowck::BorrowckResult {
        glyim_borrowck::check_borrows(ctx, body)
    }
    /// optimize.
    pub fn optimize(
        ctx: &glyim_type::TyCtx,
        body: &std::sync::Arc<glyim_mir::Body>,
    ) -> glyim_mir::Body {
        glyim_opt::optimize(ctx, body).body
    }
}
