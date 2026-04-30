pub use miette::{self, Diagnostic, LabeledSpan, Report, Severity, SourceSpan};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        assert!(start <= end);
        Self { start, end }
    }
    pub fn len(&self) -> usize {
        self.end - self.start
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Convert a Span into a miette SourceSpan.
pub fn into_source_span(span: Span) -> SourceSpan {
    SourceSpan::from(span.start..span.end)
}
