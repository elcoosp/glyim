use crate::builder::MirBuilder;
use glyim_span::Span;

/// Extension trait for MirBuilder to allow splitting impls across modules.
pub trait TerminatorExt {
    /// terminate.
    fn terminate(&mut self, kind: glyim_mir::TerminatorKind, span: Span);
}

impl<'a> TerminatorExt for MirBuilder<'a> {
    fn terminate(&mut self, kind: glyim_mir::TerminatorKind, span: Span) {
        if let Some(bb) = self.current_block {
            // A block that branches to itself is an infinite loop with no
            // exit. It can only arise when the current block was redirected
            // into the *target* block before its own terminator was written
            // (e.g. a diverging `-> !` body that never terminated, leaving a
            // stale merge block). Downgrade it to `Unreachable` so the
            // program aborts at the runtime panic entry instead of spinning
            // forever — `assert!(false)` used to hang for exactly this
            // reason.
            let kind = match kind {
                glyim_mir::TerminatorKind::Goto { target } if target == bb => {
                    glyim_mir::TerminatorKind::Unreachable
                }
                other => other,
            };
            self.basic_blocks[bb].terminator = glyim_mir::Terminator {
                kind,
                source_info: glyim_mir::SourceInfo::new(span),
            };
            self.current_block = None;
        }
    }
}
