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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_new_valid() {
        let s = Span::new(0, 5);
        assert_eq!(s.start, 0);
        assert_eq!(s.end, 5);
    }

    #[test]
    fn span_new_equal_bounds_is_empty() {
        let s = Span::new(3, 3);
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    #[should_panic]
    fn span_new_panic_on_inverted() {
        let _ = Span::new(5, 0);
    }

    #[test]
    fn span_len() {
        assert_eq!(Span::new(0, 10).len(), 10);
        assert_eq!(Span::new(5, 8).len(), 3);
    }

    #[test]
    fn span_is_empty_false() {
        assert!(!Span::new(0, 1).is_empty());
        assert!(!Span::new(100, 200).is_empty());
    }

    #[test]
    fn into_source_span_basic() {
        let ss = into_source_span(Span::new(3, 7));
        assert_eq!(ss.offset(), 3);
        assert_eq!(ss.len(), 4);
    }

    #[test]
    fn into_source_span_empty() {
        let ss = into_source_span(Span::new(10, 10));
        assert_eq!(ss.offset(), 10);
        assert_eq!(ss.len(), 0);
    }
}
