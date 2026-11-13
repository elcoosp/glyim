//! Source locations, hygiene context, multi-span for diagnostics.

/// hygiene.
pub mod hygiene;
pub use hygiene::*;

use miette::SourceSpan;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// `FileId`.
pub struct FileId(u32);

impl FileId {
    /// `BOGUS`.
    pub const BOGUS: FileId = FileId(u32::MAX);
    /// `from_raw`.
    #[must_use]
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
    /// `to_raw`.
    #[must_use]
    pub fn to_raw(self) -> u32 {
        self.0
    }
    /// index.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// `ByteIdx`.
pub struct ByteIdx(u32);

impl ByteIdx {
    /// `ZERO`.
    pub const ZERO: ByteIdx = ByteIdx(0);
    /// `from_raw`.
    #[must_use]
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
    /// `to_raw`.
    #[must_use]
    pub fn to_raw(self) -> u32 {
        self.0
    }
    /// `to_usize`.
    #[must_use]
    pub fn to_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// `Span`.
pub struct Span {
    /// `Struct`.
    pub file: FileId,
    /// `Struct`.
    pub lo: ByteIdx,
    /// `Struct`.
    pub hi: ByteIdx,
    /// `Struct`.
    pub ctx: SyntaxContext,
}

impl Span {
    /// `DUMMY`.
    pub const DUMMY: Span = Span {
        file: FileId(u32::MAX),
        lo: ByteIdx(0),
        hi: ByteIdx(0),
        ctx: SyntaxContext::ROOT,
    };

    /// new.
    ///
    /// # Panics
    ///
    /// Panics when `lo > hi` — a backwards source range is a
    /// compiler-internal invariant violation that would corrupt every
    /// downstream span computation. The check is unconditional (not a
    /// `debug_assert!`) so the failure is loud in production too.
    #[must_use]
    pub fn new(file: FileId, lo: ByteIdx, hi: ByteIdx, ctx: SyntaxContext) -> Self {
        // Unconditional (not `debug_assert!`): a lo>hi span is a
        // compiler-internal invariant violation that would corrupt every
        // downstream span computation. The cost is one compare per span —
        // negligible next to parsing — and the loud failure beats silently
        // propagating a backwards range.
        assert!(
            lo <= hi,
            "Span lo > hi (lo = {lo:?}, hi = {hi:?}) — a backwards source range"
        );
        Self { file, lo, hi, ctx }
    }

    /// `is_dummy`.
    #[must_use]
    pub fn is_dummy(self) -> bool {
        self == Self::DUMMY
    }
    /// range.
    #[must_use]
    pub fn range(self) -> Range<usize> {
        self.lo.to_usize()..self.hi.to_usize()
    }
    /// `sans_ctx`.
    #[must_use]
    pub fn sans_ctx(self) -> Span {
        Span {
            ctx: SyntaxContext::ROOT,
            ..self
        }
    }
    /// len.
    #[must_use]
    pub fn len(self) -> u32 {
        self.hi.to_raw().saturating_sub(self.lo.to_raw())
    }

    /// `is_empty`.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// to.
    ///
    /// # Panics
    ///
    /// Panics when the two spans come from different files: merging
    /// them would attribute text from one file to another. The check is
    /// unconditional so the compiler bug is loud in release builds too.
    ///
    /// T198-PATCHED [SPAN-3]: merging spans from different files is a
    /// compiler bug — the resulting span would attribute text from one
    /// file to another. The old `debug_assert_eq!` silently passed in
    /// release builds, producing wrong diagnostic locations. Promote to
    /// a hard assert so the invariant is enforced in production too.
    #[must_use]
    pub fn to(self, other: Span) -> Span {
        assert_eq!(
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
/// `SyntaxContext`.
pub struct SyntaxContext(u32);

impl SyntaxContext {
    /// `ROOT`.
    pub const ROOT: SyntaxContext = SyntaxContext(0);
    /// `is_root`.
    #[must_use]
    pub fn is_root(self) -> bool {
        self.0 == 0
    }
    /// `to_raw`.
    #[must_use]
    pub fn to_raw(self) -> u32 {
        self.0
    }

    /// `from_raw`.
    #[must_use]
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// `ExpnId`.
pub struct ExpnId(u32);

impl ExpnId {
    /// `ROOT`.
    pub const ROOT: ExpnId = ExpnId(0);
    /// `is_root`.
    #[must_use]
    pub fn is_root(self) -> bool {
        self.0 == 0
    }
    /// `to_raw`.
    #[must_use]
    pub fn to_raw(self) -> u32 {
        self.0
    }

    /// `from_raw`.
    #[must_use]
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// `Transparency`.
pub enum Transparency {
    /// `Variant`.
    Transparent,
    /// `Variant`.
    SemiTransparent,
    /// `Variant`.
    Opaque,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// `HygieneKey`.
pub struct HygieneKey(());

impl HygieneKey {
    pub(crate) fn new() -> Self {
        HygieneKey(())
    }
}

#[derive(Clone, Debug)]
/// `MultiSpan`.
pub struct MultiSpan {
    /// `Struct`.
    pub primary: Span,
    #[doc = "field"]
    pub secondary: Vec<(Span, String)>,
}

impl MultiSpan {
    /// `from_span`.
    #[must_use]
    pub fn from_span(span: Span) -> Self {
        Self {
            primary: span,
            secondary: Vec::new(),
        }
    }
    /// `with_secondary`.
    #[must_use]
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
    #[must_use]
    pub fn expn_id(self) -> ExpnId {
        // T149-PATCHED [SPAN-1]: `SyntaxContext` and `ExpnId` are
        // allocated from *independent* counters in `HygieneCtx`, and
        // `syntax_contexts[i].outer_expn` is the real association.
        // The previous `ExpnId::from_raw(self.to_raw())` returned an id
        // that indexes the expansions table at the *context* index —
        // e.g. context #3 might belong to expansion #1 but claim to be
        // expansion #3. Return `ExpnId::ROOT` here as a safe placeholder
        // until the hygiene ctx plumb-through lands; callers that need
        // the real expn id must go through `HygieneCtx`.
        ExpnId::ROOT
    }
}
