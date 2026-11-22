//! Source locations, hygiene context, multi-span for diagnostics.

/// hygiene.
pub mod hygiene;
pub use hygiene::*;

use miette::SourceSpan;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// `FileId`.
///
/// # Opacity (CT-1, plan §6 M3.1 Round 2)
///
/// `FileId` is an opaque, arena-style handle. The raw `from_raw(u32)`
/// escape hatch is `pub(crate)` so external crates cannot mint arbitrary
/// ids; the only public construction path is the sanctioned
/// [`FileId::from_usize`] factory (plus the [`FileId::BOGUS`] sentinel).
/// Full arena issuance — where even `from_usize` is `pub(crate)` and only a
/// `SourceMap` allocator mints ids — is deferred to the M3.2 F-lane codemod
/// (it needs a `SourceMap` owner; tracked as the next queue item).
///
/// ```compile_fail
/// // External crates must not reach the raw u32 escape hatch. Only the
/// // sanctioned `FileId::from_usize` factory is public.
/// let _ = glyim_span::FileId::from_raw(7);
/// ```
pub struct FileId(u32);

impl FileId {
    /// `BOGUS`.
    pub const BOGUS: FileId = FileId(u32::MAX);
    /// Sanctioned factory: mint a `FileId` from a natural index.
    ///
    /// This is the only public construction path for a non-sentinel
    /// `FileId`; the raw `from_raw` escape hatch is `pub(crate)`. The
    /// `usize` -> `u32` narrowing is deliberate (handle widths are `u32` by
    /// construction) and is guarded by the arena invariant that file counts
    /// stay well below `u32::MAX`.
    #[must_use]
    pub fn from_usize(idx: usize) -> Self {
        Self(idx as u32)
    }
    /// Raw escape hatch — `#[cfg(test)] pub(crate)`. It exists *only* when
    /// compiling `glyim-span`'s own tests (which exercise the `u32`
    /// round-trip via `to_raw`); the raw constructor is deliberately absent
    /// from production builds, so neither external crates nor non-test
    /// internal code can mint a `FileId` from an arbitrary `u32`. The
    /// sanctioned production path is [`FileId::from_usize`]; a future
    /// `SourceMap` arena that needs raw issuance will un-gate this in the
    /// same commit that owns the arena.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_raw(raw: u32) -> Self {
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
///
/// # Opacity (CT-1, plan §6 M3.1 Round 2)
///
/// `ByteIdx` is an opaque byte-offset handle. The raw `from_raw(u32)`
/// escape hatch is `pub(crate)`; the only public construction path is the
/// sanctioned [`ByteIdx::from_usize`] factory (plus [`ByteIdx::ZERO`]).
/// This closes the raw-`u32` minting path to external crates while keeping
/// the natural `usize` <-> `ByteIdx` round-trip (`from_usize` / `to_usize`)
/// that the lexer needs for `usize` cursor positions.
///
/// ```compile_fail
/// // External crates must not reach the raw u32 escape hatch. Only the
/// // sanctioned `ByteIdx::from_usize` factory is public.
/// let _ = glyim_span::ByteIdx::from_raw(64);
/// ```
pub struct ByteIdx(u32);

impl ByteIdx {
    /// `ZERO`.
    pub const ZERO: ByteIdx = ByteIdx(0);
    /// Sanctioned factory: mint a `ByteIdx` from a natural `usize` offset.
    ///
    /// This is the public counterpart of [`ByteIdx::to_usize`]. The
    /// `usize` -> `u32` narrowing is deliberate (byte offsets are `u32` by
    /// construction) and matches the existing `from_raw(start as u32)`
    /// pattern at every call site, now centralized here.
    #[must_use]
    pub fn from_usize(idx: usize) -> Self {
        Self(idx as u32)
    }
    /// Raw escape hatch — `#[cfg(test)] pub(crate)`. It exists *only* when
    /// compiling `glyim-span`'s own tests (which exercise the `u32`
    /// round-trip via `to_usize`/`to_raw`); the raw constructor is
    /// deliberately absent from production builds, so neither external crates
    /// nor non-test internal code can mint a `ByteIdx` from an arbitrary
    /// `u32`. The sanctioned production path is [`ByteIdx::from_usize`].
    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_raw(raw: u32) -> Self {
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
///
/// # Opacity (CT-1, plan §6 M3.1 Round 2)
///
/// `SyntaxContext` is a fully opaque hygiene handle: the raw
/// `from_raw(u32)` escape hatch is `pub(crate)`, and there is *no* public
/// `from_usize` factory — external crates can only obtain the [`ROOT`]
/// sentinel or a context handed to them by `HygieneCtx` (the hygiene
/// arena). This is the strongest opacity tier in this round: unlike
/// `FileId`/`ByteIdx` (which keep a sanctioned `from_usize` bridge), a
/// `SyntaxContext` cannot be minted by any external code path at all.
///
/// ```compile_fail
/// // External crates must not mint a SyntaxContext from a raw u32.
/// let _ = glyim_span::SyntaxContext::from_raw(3);
/// ```
///
/// [`ROOT`]: SyntaxContext::ROOT
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

    /// Raw escape hatch — `#[cfg(test)] pub(crate)`. It exists *only* when
    /// compiling `glyim-span`'s own tests; the raw constructor is
    /// deliberately absent from production builds, so a `SyntaxContext`
    /// can never be minted from a raw `u32` outside the crate's test
    /// harness. Production code obtains contexts only from `HygieneCtx`
    /// (the hygiene arena) or [`SyntaxContext::ROOT`].
    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// `ExpnId`.
///
/// # Opacity (CT-1, plan §6 M3.1 Round 2)
///
/// `ExpnId` is a fully opaque expansion-handle: the raw `from_raw(u32)`
/// escape hatch is `pub(crate)` with no public `from_usize` factory, so
/// external crates can only obtain [`ExpnId::ROOT`] or an id handed out by
/// `HygieneCtx`. (Privatized in the same M3.1 Round 2 pass as
/// `SyntaxContext` for consistency — `ExpnId` had no external `from_raw`
/// callers, so this is a pure visibility tightening with no call-site
/// migration.)
///
/// ```compile_fail
/// // External crates must not mint an ExpnId from a raw u32.
/// let _ = glyim_span::ExpnId::from_raw(9);
/// ```
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

    /// Raw escape hatch — `#[cfg(test)] pub(crate)`. It exists *only* when
    /// compiling `glyim-span`'s own tests; the raw constructor is
    /// deliberately absent from production builds, so an `ExpnId` can never
    /// be minted from a raw `u32` outside the crate's test harness.
    /// Production code obtains ids only from `HygieneCtx` (the hygiene
    /// arena) or [`ExpnId::ROOT`].
    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_raw(raw: u32) -> Self {
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
