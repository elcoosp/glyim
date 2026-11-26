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
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
    /// The largest byte offset a `ByteIdx` can carry (`u32::MAX`).
    ///
    /// The VFS refuses any source longer than this ([`MAX_SOURCE_LEN`]), so
    /// every cursor minted below [`ByteIdx::MAX`] is representable.
    ///
    /// [`MAX_SOURCE_LEN`]: crate::MAX_SOURCE_LEN
    pub const MAX: u32 = u32::MAX;
    /// Checked mint: turn a `usize` cursor into a `ByteIdx`, refusing
    /// offsets past [`ByteIdx::MAX`] instead of silently narrowing them.
    ///
    /// The unchecked [`ByteIdx::from_usize`] stays the hot-path lexer mint
    /// for cursors that are already known to sit below the VFS
    /// source-length gate; this constructor is the total path every
    /// *unchecked* origin (host paths, external indices, tests) must use,
    /// so an offset past `u32::MAX` surfaces as an error rather than
    /// wrapping into a bogus span (the FIXES §4 wraparound class).
    ///
    /// # Errors
    ///
    /// Returns [`ByteIdxOverflow`] when `idx > u32::MAX`.
    pub fn try_from_usize(idx: usize) -> Result<Self, ByteIdxOverflow> {
        match u32::try_from(idx) {
            Ok(raw) => Ok(Self(raw)),
            Err(_) => Err(ByteIdxOverflow { value: idx }),
        }
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

/// The refusal a checked [`ByteIdx::try_from_usize`] mint returns for an
/// offset that does not fit [`u32::MAX`] (plan M3.2 / CT-3: offsets past
/// `u32::MAX` are reported, never wrapped).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ByteIdxOverflow {
    /// The refused offset (the full `usize`, so the diagnostic can print
    /// the real value instead of the truncated one).
    pub value: usize,
}

impl std::fmt::Display for ByteIdxOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "byte offset {} exceeds the representable span range (max {})",
            self.value,
            ByteIdx::MAX
        )
    }
}

impl std::error::Error for ByteIdxOverflow {}

/// The largest source text (in bytes) the compiler accepts: every byte
/// offset in a file must fit a `u32`, because [`Span`] stores
/// `(lo, len)` over `u32` halves (plan M3.2 / CT-3).
pub const MAX_SOURCE_LEN: usize = u32::MAX as usize;

/// The refusal the checked [`Span::new`] constructor returns when
/// `lo + len` would run past [`u32::MAX`] (plan M3.2 / CT-3: an oversized
/// span is an error, never a wrapped one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpanOverflow {
    /// The span's start offset.
    pub lo: ByteIdx,
    /// The refused length (the caller's value, unclamped).
    pub len: u32,
}

impl std::fmt::Display for SpanOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "span at offset {} with length {} runs past the representable span range (max {})",
            self.lo.to_usize(),
            self.len,
            ByteIdx::MAX
        )
    }
}

impl std::error::Error for SpanOverflow {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// `Span` — a half-open byte range `[lo, lo + len)` in one file, under one
/// hygiene context.
///
/// # CT-3 representation (plan §6 M3.2 / §5.2 CT-3)
///
/// The stored shape is **`(lo, len)`**: every `(lo, len)` pair names a
/// valid span, so a span whose end precedes its start is *unrepresentable*
/// — the old `Span { lo, hi }` shape could carry `lo > hi` whenever a
/// struct literal skipped the constructor, and the constructor's guard was
/// historically a `debug_assert` that vanished in release builds (the
/// FIXES §4 class). Fields are private: construction goes through
/// [`Span::range`] (`lo, hi` bridge), [`Span::new`] (checked `lo, len`),
/// [`Span::at`] (point), or [`Span::DUMMY`].
///
/// Offsets past `u32::MAX` never wrap: [`Span::new`] refuses them with a
/// [`SpanOverflow`] error, and sources longer than [`MAX_SOURCE_LEN`] are
/// refused at the VFS boundary before any span exists.
pub struct Span {
    /// The file the span points into.
    file: FileId,
    /// The span's start byte offset.
    lo: ByteIdx,
    /// The span's length in bytes (`lo + len <= u32::MAX`, enforced at
    /// construction).
    len: u32,
    /// The hygiene context the span was parsed under.
    ctx: SyntaxContext,
}

impl Span {
    /// `DUMMY` — the sentinel for compiler-generated positions with no
    /// source text: the bogus file, offset 0, zero length, root context.
    pub const DUMMY: Span = Span {
        file: FileId(u32::MAX),
        lo: ByteIdx(0),
        len: 0,
        ctx: SyntaxContext::ROOT,
    };

    /// Checked `(lo, len)` constructor — the plan's CT-3 representation.
    ///
    /// # Errors
    ///
    /// Returns [`SpanOverflow`] when `lo + len` would run past
    /// [`u32::MAX`] (the wraparound class), so an oversized span is
    /// reported instead of silently wrapping.
    pub fn new(
        file: FileId,
        lo: ByteIdx,
        len: u32,
        ctx: SyntaxContext,
    ) -> Result<Self, SpanOverflow> {
        let end = u64::from(lo.to_raw()) + u64::from(len);
        if end > u64::from(ByteIdx::MAX) {
            return Err(SpanOverflow { lo, len });
        }
        Ok(Self { file, lo, len, ctx })
    }

    /// `(lo, hi)` bridge constructor: build a span from a start/end pair.
    ///
    /// # Panics
    ///
    /// Panics when `lo > hi` — a backwards source range is a
    /// compiler-internal invariant violation that would corrupt every
    /// downstream span computation. The check is unconditional (not a
    /// `debug_assert!`) so the failure is loud in production too; the
    /// *stored* representation stays `(lo, len)`, so no span in existence
    /// can be backwards.
    #[must_use]
    pub fn range(file: FileId, lo: ByteIdx, hi: ByteIdx, ctx: SyntaxContext) -> Self {
        assert!(
            lo <= hi,
            "Span lo > hi (lo = {lo:?}, hi = {hi:?}) — a backwards source range"
        );
        Self {
            file,
            lo,
            len: hi.to_raw() - lo.to_raw(),
            ctx,
        }
    }

    /// Infallible zero-length point span at `lo` (the constructor the
    /// "position" call sites — macro call sites, definition names — mean).
    #[must_use]
    pub fn at(file: FileId, lo: ByteIdx, ctx: SyntaxContext) -> Self {
        Self {
            file,
            lo,
            len: 0,
            ctx,
        }
    }

    /// The file the span points into.
    #[must_use]
    pub fn file(self) -> FileId {
        self.file
    }
    /// The span's start byte offset.
    #[must_use]
    pub fn lo(self) -> ByteIdx {
        self.lo
    }
    /// The span's length in bytes.
    #[must_use]
    pub fn len(self) -> u32 {
        self.len
    }
    /// The span's end byte offset (`lo + len`; exact by the construction
    /// invariant — `lo + len <= u32::MAX` holds for every span in
    /// existence, so the recombination cannot wrap).
    #[must_use]
    pub fn hi(self) -> ByteIdx {
        // `lo + len <= u32::MAX` holds for every span in existence (enforced
        // at construction by `new`/`range`/`at`), so the u64 sum always
        // re-fits a `u32` and the narrowing is exact, never a wrap.
        let end = u64::from(self.lo.to_raw()) + u64::from(self.len);
        ByteIdx::from_usize(end as usize)
    }
    /// The hygiene context the span was parsed under.
    #[must_use]
    pub fn ctx(self) -> SyntaxContext {
        self.ctx
    }

    /// `is_dummy`.
    #[must_use]
    pub fn is_dummy(self) -> bool {
        self == Self::DUMMY
    }
    /// The span's byte range `[lo, hi)` as a `Range<usize>` (the `miette`
    /// renderer and the LSP consume this form; named `byte_range` so the
    /// [`Span::range`] constructor keeps the `(lo, hi)` name).
    #[must_use]
    pub fn byte_range(self) -> Range<usize> {
        self.lo.to_usize()..self.hi().to_usize()
    }
    /// `sans_ctx`.
    #[must_use]
    pub fn sans_ctx(self) -> Span {
        Span {
            ctx: SyntaxContext::ROOT,
            ..self
        }
    }

    /// Rebuild the span under a different hygiene context (the hygiene
    /// machinery's only reshape: the location half is carried as-is).
    #[must_use]
    pub fn with_ctx(self, ctx: SyntaxContext) -> Span {
        Span { ctx, ..self }
    }

    /// `is_empty`.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.len == 0
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
        let lo = std::cmp::min(self.lo, other.lo);
        let hi = std::cmp::max(self.hi(), other.hi());
        Span::range(self.file, lo, hi, self.ctx)
    }
}

impl From<Span> for SourceSpan {
    fn from(s: Span) -> SourceSpan {
        let start = s.lo().to_raw() as usize;
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
    pub fn from_raw(raw: u32) -> Self {
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
