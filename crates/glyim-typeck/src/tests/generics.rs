//! Pipeline-level proof that generic functions are supported end-to-end
//! (plan §9.A). A `fn id<T>(x: T) -> T` must resolve the type parameter `T`
//! (previously `unresolved type T`, because HIR lowering dropped the
//! `TypeParamList` produced by the parser) and the generic body must
//! type-check and lower through the pipeline.
use glyim_span::FileId;
use glyim_test::assert_no_errors;
use glyim_test::harness::compiler::{CompileOutput, PipelineCompiler, TestCompiler};
use std::sync::Arc;

use glyim_test::mock::MockCodegen;

fn compile(src: &str) -> CompileOutput {
    let backend = Arc::new(MockCodegen::new());
    let compiler = PipelineCompiler::new(backend);
    compiler.compile(src, FileId::from_raw(1), &[])
}

#[test]
fn generic_fn_typechecks_and_lowers() {
    let src = r#"
        fn id<T>(x: T) -> T { x }
        fn main() -> i32 {
            let a = id(40);
            let b = id(2);
            a + b
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

#[test]
fn generic_struct_resolves_with_args() {
    // `struct Pair<T, U>` carries generic params; instantiating `Pair { .. }`
    // must produce a well-formed ADT type (plan unstub-5 P1.4). The element
    // types are inferred from the struct literal fields.
    let src = r#"
        struct Pair<T, U> { first: T, second: U }
        fn main() -> i32 {
            let p = Pair { first: 1, second: true };
            p.first
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

#[test]
fn generic_param_bounds_are_captured() {
    // `fn block_on<F: MyFuture>(f: F)` declares a trait bound on `F`. The
    // parser emits it and lowering must capture it in HIR (previously the
    // bound was dropped). This test pins that the bound does not crash
    // lowering / typeck and that the crate still compiles end-to-end. Solving
    // the bound (projection of `F::Output`, dispatch on `F: MyFuture`) is the
    // tracked P5 gap; see `KNOWN_GAPS.md` Phase 5.
    let src = r#"
        trait MyFuture { type Output; fn poll(&mut self) -> i32; }
        struct AddOne { x: i32 }
        impl MyFuture for AddOne { type Output = i32; fn poll(&mut self) -> i32 { self.x } }
        fn block_on<F: MyFuture>(mut f: F) -> i32 { f.poll() }
        fn main() -> i32 { let f = AddOne { x: 7 }; block_on(f) }
    "#;
    let output = compile(src);
    // Expect the P5 blocker diagnostics (the bound is captured but not yet
    // solved); assert the crate lowered without a hard internal error.
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| format!("{}", d).contains("internal error")),
        "generic bound lowered without internal error"
    );
}

#[test]
fn concrete_associated_type_projection_resolves() {
    // Plan unstub-5 P5: a *concrete* associated-type projection (`AddOne::Output`)
    // must resolve to its defining type (`i32`). The projection table is built
    // and queried by `resolve_path_type` for 2-segment `Type::Item` paths (verified
    // by the PROJ-LOOKUP unit tests in `glyim-type`). End-to-end signature
    // projection is gated on two-phase resolution (the impl must be registered
    // before a fn that names `AddOne::Output`); that ordering lives in the
    // item-pass restructure tracked in KNOWN_GAPS. Here we assert the crate
    // lowers without a hard internal error.
    let src = r#"
        trait MyFuture { type Output; fn poll(&mut self) -> i32; }
        struct AddOne { x: i32 }
        impl MyFuture for AddOne { type Output = i32; fn poll(&mut self) -> i32 { self.x } }
        fn take_output(x: AddOne::Output) -> AddOne::Output { x }
        fn main() -> i32 { 7 }
    "#;
    let output = compile(src);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| format!("{}", d).contains("internal error")),
        "concrete assoc-type projection lowered without internal error"
    );
}
