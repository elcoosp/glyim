//! Source locations, hygiene context, multi-span for diagnostics.

/// hygiene.
pub mod hygiene;
pub use hygiene::*;

use miette::SourceSpan;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// FileId.
pub struct FileId(u32);

impl FileId {
    /// BOGUS.
    pub const BOGUS: FileId = FileId(u32::MAX);
    /// from_raw.
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
    /// to_raw.
    pub fn to_raw(self) -> u32 {
        self.0
    }
    /// index.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// ByteIdx.
pub struct ByteIdx(u32);

impl ByteIdx {
    /// ZERO.
    pub const ZERO: ByteIdx = ByteIdx(0);
    /// from_raw.
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
    /// to_raw.
    pub fn to_raw(self) -> u32 {
        self.0
    }
    /// to_usize.
    pub fn to_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Span.
pub struct Span {
    /// Struct.
    pub file: FileId,
    /// Struct.
    pub lo: ByteIdx,
    /// Struct.
    pub hi: ByteIdx,
    /// Struct.
    pub ctx: SyntaxContext,
}

impl Span {
    /// DUMMY.
    pub const DUMMY: Span = Span {
        file: FileId(u32::MAX),
        lo: ByteIdx(0),
        hi: ByteIdx(0),
        ctx: SyntaxContext::ROOT,
    };

    /// new.
    pub fn new(file: FileId, lo: ByteIdx, hi: ByteIdx, ctx: SyntaxContext) -> Self {
        debug_assert!(lo <= hi, "Span lo > hi");
        Self { file, lo, hi, ctx }
    }

    /// is_dummy.
    pub fn is_dummy(self) -> bool {
        self == Self::DUMMY
    }
    /// range.
    pub fn range(self) -> Range<usize> {
        self.lo.to_usize()..self.hi.to_usize()
    }
    /// sans_ctx.
    pub fn sans_ctx(self) -> Span {
        Span {
            ctx: SyntaxContext::ROOT,
            ..self
        }
    }
    /// len.
    pub fn len(self) -> u32 {
        self.hi.to_raw().saturating_sub(self.lo.to_raw())
    }

    #[allow(clippy::len_without_is_empty)]
    /// is_empty.
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// to.
    pub fn to(self, other: Span) -> Span {
        debug_assert_eq!(
            self.file, other.file,
            "Cannot merge spans from different files"
        );
        Span {
            file: self.file,
            lo: std::cmp::min(self.lo, other.lo),
            hi: std::cmp::max(self.hi, other.hi),
            ctx: self.ctx,
        }
    }
}

impl From<Span> for SourceSpan {
    fn from(s: Span) -> SourceSpan {
        let start = s.lo.to_raw() as usize;
        let length = s.len() as usize;
        SourceSpan::new(start.into(), length)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// SyntaxContext.
pub struct SyntaxContext(u32);

impl SyntaxContext {
    /// ROOT.
    pub const ROOT: SyntaxContext = SyntaxContext(0);
    /// is_root.
    pub fn is_root(self) -> bool {
        self.0 == 0
    }
    /// to_raw.
    pub fn to_raw(self) -> u32 {
        self.0
    }
    #[allow(dead_code)]
    #[allow(dead_code)]
    /// from_raw.
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// ExpnId.
pub struct ExpnId(u32);

impl ExpnId {
    /// ROOT.
    pub const ROOT: ExpnId = ExpnId(0);
    /// is_root.
    pub fn is_root(self) -> bool {
        self.0 == 0
    }
    /// to_raw.
    pub fn to_raw(self) -> u32 {
        self.0
    }
    #[allow(dead_code)]
    /// from_raw.
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Transparency.
pub enum Transparency {
    /// Variant.
    Transparent,
    /// Variant.
    SemiTransparent,
    /// Variant.
    Opaque,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// HygieneKey.
pub struct HygieneKey(());

impl HygieneKey {
    pub(crate) fn new() -> Self {
        HygieneKey(())
    }
}

#[derive(Clone, Debug)]
/// MultiSpan.
pub struct MultiSpan {
    /// Struct.
    pub primary: Span,
    #[doc = "field"]
    pub secondary: Vec<(Span, String)>,
}

impl MultiSpan {
    /// from_span.
    pub fn from_span(span: Span) -> Self {
        Self {
            primary: span,
            secondary: Vec::new(),
        }
    }
    /// with_secondary.
    pub fn with_secondary(mut self, span: Span, label: impl Into<String>) -> Self {
        self.secondary.push((span, label.into()));
        self
    }
}

impl From<Span> for MultiSpan {
    fn from(span: Span) -> Self {
        Self::from_span(span)
    }
}

#[cfg(test)]
mod tests;
impl SyntaxContext {
    /// Returns the expansion ID associated with this syntax context.
    /// For root contexts, returns `ExpnId::ROOT`.
    pub fn expn_id(self) -> ExpnId {
        ExpnId::from_raw(self.to_raw())
    }
}
#[cfg(test)]
impl ExpnId {
    /// Test-only constructor to create an ExpnId from a raw u32.
    pub fn test_from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

#[cfg(test)]
impl SyntaxContext {
    /// Test-only constructor to create a SyntaxContext from a raw u32.
    pub fn test_from_raw(raw: u32) -> Self {
        Self::from_raw(raw)
    }
}
