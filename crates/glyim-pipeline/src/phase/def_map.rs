//! Expand + `DefMap` phase (M5.1): expand builtin/proc macros and build the
//! crate definition map, extracting `prepare_compilation`'s Expand + `DefMap`
//! stages into a [`CompilerPhase`].
use crate::Phase;
use crate::phase::{CompilerPhase, PhaseCtx, extract_diagnostics};
use glyim_def_map::CrateDefMap;
use glyim_diag::CompResult;
use glyim_proc_macro::Registry;
use glyim_span::FileId;
use glyim_syntax::SyntaxNode;

/// Output of [`DefMapPhase`]: the expanded syntax root and the crate def map.
pub struct DefMapOutput {
    /// The macro-expanded crate root.
    pub expanded_root: SyntaxNode,
    /// The crate definition map.
    pub def_map: CrateDefMap,
}

/// Expand macros and build the crate definition map. Extracted from
/// `prepare_compilation`'s Expand + `DefMap` stages.
pub struct DefMapPhase<'a> {
    /// The parse root to expand (`LexParseOutput::parse_result.root`).
    pub root: &'a SyntaxNode,
    /// The file id anchoring `file!`/`include!`/`line!`.
    pub file_id: FileId,
    /// Optional proc-macro registry to register on the expander.
    pub proc_registry: Option<&'a Registry>,
}

impl CompilerPhase for DefMapPhase<'_> {
    type Output = DefMapOutput;
    fn name(&self) -> &'static str {
        "expand-def-map"
    }
    fn run(&self, ctx: &mut PhaseCtx) -> CompResult<Self::Output> {
        let expanded_root = {
            let _guard = ctx.recorder.enter(Phase::Expand);
            let mut hygiene = glyim_span::HygieneCtx::new();
            let mut expander = glyim_meta::Expander::new(&mut hygiene);
            // Anchor `file!`/`include!`/`line!` to the real source and let
            // `include!` resolve relative paths against the calling file.
            // Without these the expander sees `FileId::BOGUS` and no VFS, so
            // a relative `include!("items.g")` cannot find its target.
            expander.set_source_file(self.file_id);
            expander.set_vfs(ctx.db.vfs());
            if let Some(reg) = self.proc_registry
                && !reg.is_empty()
            {
                expander.with_proc_registry(Some(reg));
            }
            let (expanded, expand_diags) = expander.expand_crate(self.root);
            ctx.sink.borrow_mut().extend(expand_diags);
            if ctx.sink.borrow().has_errors() {
                return Err(extract_diagnostics(ctx));
            }
            expanded
        };
        let (def_map, def_diagnostics) = {
            let _guard = ctx.recorder.enter(Phase::DefMap);
            glyim_def_map::build_def_map(&expanded_root, ctx.db.krate(), ctx.db.interner().clone())
        };
        ctx.sink.borrow_mut().extend(def_diagnostics);
        if ctx.sink.borrow().has_errors() {
            return Err(extract_diagnostics(ctx));
        }
        Ok(DefMapOutput {
            expanded_root,
            def_map,
        })
    }
}
