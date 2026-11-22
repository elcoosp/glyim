//! Lex + Parse phase (M5.1): load the crate source (flattening external
//! `mod foo;`) and parse it, extracting the front half of
//! `prepare_compilation`'s Load/Parse stages into a [`CompilerPhase`].
use crate::Phase;
use crate::phase::{CompilerPhase, PhaseCtx, extract_diagnostics};
use glyim_diag::CompResult;
use glyim_frontend::ParseResult;
use glyim_span::FileId;
use std::path::Path;
use std::sync::Arc;

/// Output of [`LexParsePhase`]: the loaded file id, source text, and parse
/// result. The parse root feeds the successor `DefMapPhase`'s expander.
pub struct LexParseOutput {
    /// The VFS file id of the (module-flattened) source.
    pub file_id: FileId,
    /// The flattened crate source.
    pub source: Arc<str>,
    /// The parse result (root + diagnostics).
    pub parse_result: ParseResult,
}

/// Load a crate's source and parse it to a [`ParseResult`]. Extracted from
/// `prepare_compilation`'s Load + Parse stages.
pub struct LexParsePhase<'a> {
    /// The source file to compile.
    pub path: &'a Path,
}

impl CompilerPhase for LexParsePhase<'_> {
    type Output = LexParseOutput;
    fn name(&self) -> &'static str {
        "lex-parse"
    }
    fn run(&self, ctx: &mut PhaseCtx) -> CompResult<Self::Output> {
        let (file_id, source) = {
            let _guard = ctx.recorder.enter(Phase::Load);
            match crate::load_crate_source(ctx.db, self.path) {
                Ok(pair) => pair,
                Err(mod_diags) => {
                    ctx.sink.borrow_mut().extend(mod_diags);
                    return Err(extract_diagnostics(ctx));
                }
            }
        };
        let parse_result = {
            let _guard = ctx.recorder.enter(Phase::Parse);
            glyim_frontend::parse_to_syntax(&source, file_id)
        };
        ctx.sink
            .borrow_mut()
            .extend(parse_result.diagnostics.clone());
        if ctx.sink.borrow().has_errors() {
            return Err(extract_diagnostics(ctx));
        }
        Ok(LexParseOutput {
            file_id,
            source,
            parse_result,
        })
    }
}
