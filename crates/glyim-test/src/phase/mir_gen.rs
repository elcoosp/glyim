/// `MirGenTester`.
pub struct MirGenTester;

impl MirGenTester {
    /// `lower_body`.
    pub fn lower_body(
        ctx: &mut dyn glyim_lower::LowerCtx,
        typed: &glyim_typeck::TypedBody,
    ) -> Result<glyim_mir::Body, Vec<glyim_diag::GlyimDiagnostic>> {
        // Plan M5.2: the harness lowers through the typed boundary too —
        // only verified bodies can reach MIR here.
        let result = glyim_lower::lower_typed_body(ctx, typed);
        if result.diagnostics.is_empty() {
            Ok(result.body)
        } else {
            Err(result.diagnostics)
        }
    }
    /// `check_borrows`.
    pub fn check_borrows(
        ctx: &dyn glyim_borrowck::BorrowckCtx,
        body: &glyim_mir::Body,
    ) -> glyim_borrowck::BorrowckResult {
        glyim_borrowck::check_borrows(ctx, body)
    }
    /// optimize.
    #[must_use]
    pub fn optimize(
        ctx: &glyim_type::TyCtx,
        body: &std::sync::Arc<glyim_mir::Body>,
    ) -> glyim_mir::Body {
        // The harness keeps its own reference, so the copy-on-write entry
        // deep-copies exactly once here (M2.7: `optimize` mutates in place
        // through `Arc::make_mut`; a shared input pays the copy, a unique
        // one pays nothing).
        glyim_opt::optimize(ctx, std::sync::Arc::clone(body))
            .expect("MIR optimization failed: body failed validation")
            .body
    }
}
