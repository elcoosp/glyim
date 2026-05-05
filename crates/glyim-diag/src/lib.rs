pub use miette::{self, Diagnostic, LabeledSpan, Report, Severity, SourceSpan};
use std::sync::{LazyLock, Mutex};

/// Metadata for a macro expansion that produced a span.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MacroExpansion {
    /// Where the macro was invoked (user code)
    pub call_site: Span,
    /// Where the macro was defined (macro author's code)
    pub def_site: Span,
    /// Name of the macro
    pub macro_name: String,
    /// For nested expansions, the parent expansion ID
    pub parent: Option<u32>,
}

/// Table of macro expansion records, indexed by Span.expansion_id.
pub static MACRO_EXPANSION_TABLE: LazyLock<Mutex<Vec<MacroExpansion>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    /// Index into MACRO_EXPANSION_TABLE, or None if not from a macro
    pub expansion_id: Option<u32>,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        assert!(start <= end);
        Self {
            start,
            end,
            expansion_id: None,
        }
    }

    pub fn with_expansion(
        start: usize,
        end: usize,
        call_site: Span,
        def_site: Span,
        macro_name: String,
        parent: Option<u32>,
    ) -> Self {
        assert!(start <= end);
        let mut table = MACRO_EXPANSION_TABLE.lock().unwrap();
        let id = table.len() as u32;
        table.push(MacroExpansion {
            call_site,
            def_site,
            macro_name,
            parent,
        });
        Self {
            start,
            end,
            expansion_id: Some(id),
        }
    }

    pub fn expansion(&self) -> Option<MacroExpansion> {
        self.expansion_id
            .map(|id| MACRO_EXPANSION_TABLE.lock().unwrap()[id as usize].clone())
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Walk the expansion chain and return the original (non‑macro) source span.
    pub fn original_source(&self) -> Span {
        if let Some(exp) = self.expansion() {
            exp.call_site.original_source()
        } else {
            *self
        }
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
