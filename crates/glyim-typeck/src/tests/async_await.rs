//! Tests for async/await keyword recognition (S17-T05 / plan §6.1).
//!
//! `async`/`await` are now real syntax keywords (`KwAsync`/`KwAwait` in
//! `glyim_syntax::SyntaxKind`), lexed and lowered into `FnItem::is_async`.
//! These tests document the post-§6.1 behavior: the keywords are recognized
//! and no longer surface spurious "unsupported" diagnostics. (Full `async`
//! desugaring into a `Future` state machine remains a separate design-doc
//! subsystem and is intentionally not asserted here.)
use glyim_diag::{ErrorCategory, ErrorCode};
use glyim_test::phase::AnalysisTester;

#[test]
fn s17_t05_async_fn_recognized_without_error() {
    // `async fn` is now a recognized declaration form (lowered to
    // `FnItem { is_async: true }` by `lower_fn_def`); it must not produce
    // lex/parse/def-map diagnostics at this phase.
    let source = r#"
async fn main() {
    let x = 42;
}
"#;
    let result = AnalysisTester::new(source).run_def_map();
    let all_diags = [
        &result.lex_diagnostics[..],
        &result.parse_diagnostics[..],
        &result.def_map_diagnostics[..],
    ]
    .concat();
    assert!(
        all_diags.is_empty(),
        "async fn should be recognized without diagnostics, got: {:?}",
        all_diags
    );
}

#[test]
fn s17_t05_await_keyword_lexed_without_error() {
    // `await` is now a recognized keyword (`KwAwait`), so it must not produce
    // an "unknown identifier" lex error. (Parsing `.await` into a full await
    // expression is out of scope for plan §6.1 — only the keyword wiring is
    // implemented here.)
    let source = r#"
fn main() {
    let x = some_future;
    await;
}
"#;
    let result = AnalysisTester::new(source).run_def_map();
    assert!(
        result.lex_diagnostics.is_empty(),
        "await must be lexed as a keyword without errors, got: {:?}",
        result.lex_diagnostics
    );
}

use glyim_span::FileId;
use glyim_test::assert_no_errors;
use glyim_test::harness::compiler::{CompileOutput, PipelineCompiler, TestCompiler};
use glyim_test::mock::MockCodegen;
use std::sync::Arc;

fn compile(src: &str) -> CompileOutput {
    let backend = Arc::new(MockCodegen::new());
    let compiler = PipelineCompiler::new(backend);
    compiler.compile(src, FileId::from_raw(1), &[])
}

/// Drive a real `async fn` + `.await` program through the full
/// `PipelineCompiler` (parser → lower → async desugar → typeck). The
/// desugar rewrites `async fn` into a `Future` state machine and `.await`
/// into a poll `Match`; the program defines `Future`/`Poll`/`block_on` in the
/// same crate. It must compile with ZERO diagnostics.
#[test]
fn desugar_async_fn_compiles() {
    let src = r#"
        enum Poll<T> { Ready(T), Pending }
        trait Future {
            type Output;
            fn poll(&mut self) -> Poll<Self::Output>;
        }
        fn block_on<F: Future>(mut f: F) -> F::Output {
            loop {
                match f.poll() {
                    Poll::Ready(v) => return v,
                    Poll::Pending => { }
                }
            }
        }
        async fn add_one(x: i32) -> i32 { x + 1 }
        fn main() -> i32 {
            let f = add_one(41);
            block_on(f)
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

/// Two `async fn`s where one awaits the other (`nested` awaits `dep`). This is
/// the single-await shape with a *cross-future* call inside the `.await`
/// desugar: `nested`'s `poll` body references `dep`'s desugared wrapper fn.
/// Regression guard for the def-map/`LocalDefId` ordering bug where the inner
/// wrapper's `fn_sig` was only registered *after* the outer poll body was
/// type-checked, and for the method-dispatch probe emitting spurious
/// "mismatched types" diagnostics on non-matching `impl Future` candidates.
#[test]
fn nested_async_single_await_compiles() {
    let src = r#"
        enum Poll<T> { Ready(T), Pending }
        trait Future {
            type Output;
            fn poll(&mut self) -> Poll<Self::Output>;
        }
        fn block_on<F: Future>(mut f: F) -> F::Output {
            loop {
                match f.poll() {
                    Poll::Ready(v) => return v,
                    Poll::Pending => { }
                }
            }
        }
        async fn dep(x: i32) -> i32 { x }
        async fn nested(a: i32) -> i32 { let x = dep(a).await; x + 1 }
        fn main() -> i32 {
            let f = nested(5);
            block_on(f)
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

/// Multi-`.await` (>= 2 suspends, none inside a loop) now compiles cleanly
/// through the REAL `desugar_multi_async_fn` HIR state-machine transform:
/// `two(a)` desugars into a `Start`/`S0`/`..`/`Done` enum plus a `poll` body
/// that drives each suspended future and stores live locals + the in-flight
/// future across `Poll::Pending`, so the future genuinely suspends and resumes.
/// This is the M4 deliverable of GLYIM_DESTUB_PLAN Phase 3 — the shape that
/// previously fell through to the `async-v2` (error 61) diagnostic now type-
/// checks and lowers with ZERO diagnostics.
#[test]
fn multi_await_compiles_cleanly() {
    let src = r#"
        enum Poll<T> { Ready(T), Pending }
        trait Future {
            type Output;
            fn poll(&mut self) -> Poll<Self::Output>;
        }
        fn block_on<F: Future>(mut f: F) -> F::Output {
            loop {
                match f.poll() {
                    Poll::Ready(v) => return v,
                    Poll::Pending => { }
                }
            }
        }
        async fn dep(x: i32) -> i32 { x }
        async fn two(a: i32) -> i32 { let x = dep(a).await; let y = dep(x).await; x + y }
        fn main() -> i32 {
            let f = two(5);
            block_on(f)
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

/// The `async-v2` (error 61) diagnostic is the plan's paramount safety net: it
/// must still fire when the desugar CANNOT build a real state machine — i.e.
/// when a suspended future's concrete type is NOT statically nameable at the
/// HIR stage (it is not a direct call to a desugared `async fn`). Such shapes
/// must be reported as a clear compile ERROR, never silently miscompiled into
/// an infinite-`Pending` hang. This guards against regressing to the forbidden
/// silent miscompile.
#[test]
fn multi_await_non_nameable_future_emits_async_v2() {
    let src = r#"
        enum Poll<T> { Ready(T), Pending }
        trait Future {
            type Output;
            fn poll(&mut self) -> Poll<Self::Output>;
        }
        fn block_on<F: Future>(mut f: F) -> F::Output {
            loop {
                match f.poll() {
                    Poll::Ready(v) => return v,
                    Poll::Pending => { }
                }
            }
        }
        async fn dep(x: i32) -> i32 { x }
        // `f` is bound by name then awaited — the desugar cannot name its
        // concrete future type from a path, so it must emit error 61.
        async fn two(a: i32) -> i32 { let f = dep(a); let x = f.await; let y = dep(x).await; x + y }
        fn main() -> i32 {
            let f = two(5);
            block_on(f)
        }
    "#;
    let output = compile(src);
    assert!(
        output.diagnostics.iter().any(|d| {
            matches!(
                &d.code,
                ErrorCode {
                    category: ErrorCategory::Type,
                    number: 61
                }
            )
        }),
        "non-nameable multi-await future must emit the async-v2 diagnostic (error 61), got: {:?}",
        output.diagnostics
    );
}
