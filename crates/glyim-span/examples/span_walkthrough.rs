//! Span walkthrough: the source-location vocabulary every diagnostic in
//! the workspace rides on.
//!
//! `glyim-span` is one of the crates without a consumer walkthrough so far:
//! its types (`FileId`, `ByteIdx`, `Span`, `SyntaxContext`, `MultiSpan`)
//! are the plumbing under every `[T0060] msg @0..5`-style diagnostic the
//! compiler prints, but nothing shows a library consumer how the pieces
//! compose. This walkthrough builds the shapes a diagnostic author
//! actually makes: two files' worth of spans, a merge (`Span::to`), the
//! hygiene contexts that keep macro-expanded spans apart, the `miette`
//! `SourceSpan` conversion the renderer consumes, and the primary +
//! secondary `MultiSpan` a two-location diagnostic carries. Run with:
//!
//! ```text
//! cargo run -p glyim-span --example span_walkthrough
//! ```

use glyim_span::{
    ByteIdx, ExpnId, FileId, HygieneCtx, Mark, MultiSpan, Span, SyntaxContext, Transparency,
};

fn main() {
    // The compiler's file table hands out `FileId`s; a diagnostic consumer
    // receives them inside spans and maps them back to paths. `BOGUS` is
    // the sentinel for "no file" (synthetic constructs).
    let src = FileId::from_usize(0);
    let lib = FileId::from_usize(1);
    println!(
        "files: src={src:?} lib={lib:?} bogus-sentinel={:?}",
        FileId::BOGUS
    );

    // Byte offsets are `ByteIdx` handles, not raw `usize`s, so a span
    // cannot mix up a byte index with a line number by accident.
    let lo = ByteIdx::from_usize(20);
    let hi = ByteIdx::from_usize(34);
    let call_site = Span::new(src, lo, hi, SyntaxContext::ROOT);
    println!(
        "call site span: {:?} range={:?} len={} bytes, root hygiene={}",
        call_site,
        call_site.range(),
        call_site.len(),
        call_site.ctx.is_root()
    );

    // `Span::DUMMY` marks synthetic positions (compiler-generated code
    // with no source text); it is the zero-length root-context span over
    // the bogus file.
    assert!(Span::DUMMY.is_dummy());
    assert!(!call_site.is_dummy());
    println!("dummy span is the sentinel: {:?}", Span::DUMMY.file);

    // Two spans of one expression merge into the span a "this expression"
    // diagnostic points at. Merging across files is a hard panic — the
    // resulting span would attribute one file's text to another.
    let receiver = Span::new(
        src,
        ByteIdx::from_usize(20),
        ByteIdx::from_usize(26),
        SyntaxContext::ROOT,
    );
    let merged = receiver.to(call_site);
    println!(
        "merged span: range={:?} (covers the receiver and the call)",
        merged.range()
    );

    // Macro hygiene: an expanded token carries a non-root `SyntaxContext`,
    // so a resolver can tell `x` written by the user from `x` produced by a
    // macro even though both spell the same identifier. After the M3.1
    // Round 2 opacity pass a non-root `SyntaxContext` can no longer be
    // minted with `from_raw` from outside the crate — the only sanctioned
    // path is the `HygieneCtx` arena, whose `apply_mark` hands out a fresh
    // context per expansion site. (The mark's `expn_id` is `ROOT` here only
    // to keep the walkthrough self-contained; a real macro would first
    // `push_expansion` to mint a non-root `ExpnId`, which is likewise
    // arena-issued and opaque externally.)
    let mut hygiene = HygieneCtx::new();
    let macro_span = hygiene.apply_mark(
        Span::new(
            lib,
            ByteIdx::from_usize(4),
            ByteIdx::from_usize(9),
            SyntaxContext::ROOT,
        ),
        Mark {
            expn_id: ExpnId::ROOT,
            transparency: Transparency::Opaque,
        },
    );
    println!(
        "macro span hygiene: ctx={} (root={}?) associated expn={:?}",
        macro_span.ctx.to_raw(),
        macro_span.ctx.is_root(),
        macro_span.ctx.expn_id()
    );

    // The diagnostic renderer's currency is `miette`'s `SourceSpan`; a
    // `Span` converts losslessly (start offset + byte length).
    let source_span = miette::SourceSpan::from(call_site);
    println!(
        "miette SourceSpan: offset={} len={}",
        source_span.offset(),
        source_span.len()
    );

    // A two-location diagnostic carries a primary span (where the error
    // is) plus labeled secondary spans (why): the `MultiSpan` the
    // `GlyimDiagnostic` constructor takes.
    let multi = MultiSpan::from_span(call_site)
        .with_secondary(receiver, "this expression has type `i32`")
        .with_secondary(macro_span, "expanded from here");
    println!(
        "multi-span: primary range={:?}, secondaries={}",
        multi.primary.range(),
        multi
            .secondary
            .iter()
            .map(|(s, label)| format!("{:?} {label}", s.range()))
            .collect::<Vec<_>>()
            .join("; ")
    );

    // Stripping hygiene (`sans_ctx`) is what a "highlight this whole line"
    // face needs when the caret must land on user-written text regardless
    // of expansion depth.
    println!(
        "sans_ctx span hygiene is root: {}",
        macro_span.sans_ctx().ctx.is_root()
    );
}
