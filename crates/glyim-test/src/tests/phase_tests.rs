use crate::assertions::span::{assert_span_pushed, assert_spans_balanced};
use crate::*;

#[test]
fn test_pipeline_compiler_construction() {
    let backend = mock::MockCodegen::new();
    let _compiler = harness::compiler::PipelineCompiler::new(std::sync::Arc::new(backend)
        as std::sync::Arc<dyn glyim_codegen::CodegenBackend + Send + Sync>);
}

#[test]
fn test_frontend_only_compiler() {
    use harness::compiler::TestCompiler;
    let compiler = harness::compiler::FrontendOnlyCompiler;
    let output = compiler.compile("fn main() {}", glyim_span::FileId::from_raw(9999), &[]);
    assert!(output.syntax_tree.is_some());
}

#[test]
fn test_frontend_tester() {
    let trace = FrontendTester::new("fn main() {}").run();
    assert!(trace.parse_tree.is_some());
}

#[test]
fn test_mir_assert() {
    let ctx = test_frozen_ty_ctx();
    let body = glyim_mir::Body::dummy(glyim_core::def_id::DefId::new(
        glyim_core::def_id::CrateId::from_raw(0),
        glyim_core::def_id::LocalDefId::from_raw(0),
    ));
    assert_mir(&ctx, &body)
        .block_count(1)
        .local_count(1)
        .block_terminator(glyim_mir::BasicBlockIdx::from_raw(0), "Unreachable");
}

#[test]
fn test_span_assertions() {
    use crate::mock::lower_ctx::SpanOp;
    let ops = vec![SpanOp::Push(glyim_span::Span::DUMMY), SpanOp::Pop];
    assert_spans_balanced(&ops);
    assert_span_pushed(&ops, glyim_span::Span::DUMMY);
}

/// Phase 9.2: macro expansion runs *during* the live pipeline compile.
/// A builtin macro (`stringify!`) is only valid if the expander rewrites it
/// before the rest of the pipeline sees the source; without expansion the
/// `stringify!` token would be an unrecognized item and the compile would
/// fail. Asserting success therefore proves expansion is wired into the driver.
#[test]
fn test_pipeline_runs_macro_expansion_builtin() {
    use crate::harness::compiler::TestCompiler;
    let backend = mock::MockCodegen::new();
    let compiler = harness::compiler::PipelineCompiler::new(std::sync::Arc::new(backend)
        as std::sync::Arc<dyn glyim_codegen::CodegenBackend + Send + Sync>);
    let src = "fn main() { let _ = stringify!(hello world); }";
    let out = compiler.compile(src, glyim_span::FileId::from_raw(1), &[]);
    assert!(
        out.diagnostics.is_empty(),
        "builtin-macro source should compile via the live pipeline (expansion ran): {:?}",
        out.diagnostics
    );
}

/// Phase 9.2: a procedural macro dispatched through an injected
/// `glyim_proc_macro::Registry` expands *during* the live pipeline compile.
/// The registry's `mk` macro rewrites `mk!(x)` into the statement
/// `let z = 4;`, so the source `fn main() { mk!(x); }` only compiles if the
/// proc-macro expansion actually ran before type-checking.
#[test]
fn test_pipeline_runs_proc_macro_via_registry() {
    use crate::harness::compiler::TestCompiler;
    use glyim_proc_macro::Registry;
    use glyim_syntax::SyntaxKind;

    let mut registry = Registry::new();
    // `mk!(x)` -> `4;` (a bare integer-literal statement). Whitespace tokens are
    // not preserved through the expander's green reconstruction, so the output
    // tokens are chosen to need no separator (`4` followed by `;` is valid
    // glyim with no adjacent identifier collision).
    registry.register(
        "mk",
        |_input: &[(SyntaxKind, String)]| -> Vec<(SyntaxKind, String)> {
            vec![
                (SyntaxKind::IntLit, "4".to_string()),
                (SyntaxKind::Semicolon, ";".to_string()),
            ]
        },
    );

    let backend = mock::MockCodegen::new();
    let compiler = harness::compiler::PipelineCompiler::new(std::sync::Arc::new(backend)
        as std::sync::Arc<dyn glyim_codegen::CodegenBackend + Send + Sync>)
    .with_proc_registry(Some(std::sync::Arc::new(registry)));

    let src = "fn main() { mk!(x) }";
    let out = compiler.compile(src, glyim_span::FileId::from_raw(1), &[]);
    assert!(
        out.diagnostics.is_empty(),
        "proc-macro source should compile via the live pipeline (expansion ran): {:?}",
        out.diagnostics
    );
}
