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
            self.basic_blocks[bb].terminator = glyim_mir::Terminator {
                kind,
                source_info: glyim_mir::SourceInfo::new(span),
            };
            self.current_block = None;
        }
    }
}
