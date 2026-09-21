use glyim_span::FileId;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_FE_ID: AtomicU32 = AtomicU32::new(2000);

/// FrontendTester.
pub struct FrontendTester {
    source: String,
    file_id: FileId,
}

impl FrontendTester {
    /// new.
    pub fn new(source: impl Into<String>) -> Self {
        let file_id = FileId::from_raw(NEXT_FE_ID.fetch_add(1, Ordering::Relaxed));
        Self {
            source: source.into(),
            file_id,
        }
    }
    /// with_file_id.
    pub fn with_file_id(mut self, id: FileId) -> Self {
        self.file_id = id;
        self
    }
    /// run.
    pub fn run(self) -> super::CompilationTrace {
        let mut trace = super::CompilationTrace::default();
        tracing::info!(phase = "parse", file_id = self.file_id.to_raw());

        // Phase 1: Parse
        let result = glyim_frontend::parse_to_syntax(&self.source, self.file_id);
        trace.parse_diagnostics = result.diagnostics.clone();
        trace.parse_tree = Some(result.root.clone());

        if trace.parse_diagnostics.iter().any(|d| d.is_error()) {
            return trace;
        }

        // Phase 2: DefMap — build with a shared interner so that HIR lowering
        // (Phase 3) and the def-map agree on `Name` ids.
        let mut interner = glyim_core::interner::Interner::new();
        let (def_map, def_diags) = glyim_def_map::build_def_map(
            &result.root,
            glyim_core::def_id::CrateId::from_raw(0),
            interner.clone(),
        );
        trace.def_map_diagnostics = def_diags.clone();
        trace.def_map = Some(def_map.clone());

        if trace.def_map_diagnostics.iter().any(|d| d.is_error()) {
            return trace;
        }

        // Phase 3: HIR lowering — reuse the same interner.
        let (hir, hir_diags) =
            glyim_hir::pipeline_api::lower_crate_for_pipeline(&result.root, &mut interner);
        trace.typeck_diagnostics.extend(hir_diags.clone());

        if trace.typeck_diagnostics.iter().any(|d| d.is_error()) {
            return trace;
        }

        // Phase 4: Typeck
        let resolver = interner.clone();
        let ty_ctx_mut = glyim_type::TyCtxMut::new(resolver);
        let trait_ctx = glyim_solve::TraitContext::new();
        let mut solver = glyim_solve::SimpleTraitSolver::new(&trait_ctx);
        let (_ty_ctx, typeck_result) =
            glyim_typeck::typeck_crate(ty_ctx_mut, &def_map, &hir, &mut solver);
        trace
            .typeck_diagnostics
            .extend(typeck_result.diagnostics.clone());
        trace.typeck_result = Some(typeck_result);

        trace
    }
    /// parse_only.
    pub fn parse_only(self) -> glyim_frontend::ParseResult {
        glyim_frontend::parse_to_syntax(&self.source, self.file_id)
    }
}
