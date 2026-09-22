//! Trait-method static dispatch: `Trait::method(receiver, ..)` resolves to the
//! concrete impl function selected by the receiver's type (no vtable / dyn
//! object needed for concrete receivers).

use glyim_span::FileId;
use glyim_test::assert_no_errors;
use glyim_test::harness::compiler::{CompileOutput, PipelineCompiler, TestCompiler};
use glyim_test::mock::MockCodegen;
use std::sync::Arc;

fn compile(src: &str) -> CompileOutput {
    let backend = Arc::new(MockCodegen::new());
    let compiler = PipelineCompiler::new(backend);
    compiler.compile(src, std::path::Path::new("test.g"), FileId::from_raw(1), &[])
}

#[test]
fn trait_method_path_dispatch_resolves() {
    let src = r#"
        trait Animal { fn speak(&self) -> i32; }
        struct Dog;
        struct Cat;
        impl Animal for Dog { fn speak(&self) -> i32 { 1 } }
        impl Animal for Cat { fn speak(&self) -> i32 { 2 } }
        fn main() -> i32 { let d = Dog; let c = Cat; Animal::speak(&d) + Animal::speak(&c) }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

#[test]
fn unit_struct_construction_resolves() {
    let src = r#"
        struct Dog;
        struct Cat;
        fn id(x: Dog) -> Dog { x }
        fn main() -> i32 { let d = Dog; let _ = id(d); 0 }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

/// Probe the `async fn` desugaring *target* through the real pipeline:
/// a trait with an associated type + method, an impl providing it, and a
/// generic `block_on<F: MyFuture>` that drives it.
///
/// This exercises the full P5 async-desugar target through the real
/// `PipelineCompiler`: generic call return-type instantiation, associated-type
/// projection normalization (`F::Output` -> `i32`), generic-enum variant
/// patterns (`Poll::Ready(v)` binding `v: F::Output`), and `return` inside a
/// `loop`/`match` (lowered to `thir::ExprKind::Return` targeting the function
/// return place, not a spurious loop break). It now compiles with ZERO
/// diagnostics — the generic-body monomorphization + associated-type
/// projection blocker (tracked in `KNOWN_GAPS.md` Phase 5) is resolved.
#[test]
fn async_desugar_target_compiles() {
    let src = r#"
        enum Poll<T> { Ready(T), Pending }
        trait MyFuture {
            type Output;
            fn poll(&mut self) -> Poll<Self::Output>;
        }
        struct AddOne { x: i32 }
        impl MyFuture for AddOne {
            type Output = i32;
            fn poll(&mut self) -> Poll<i32> { Poll::Ready(self.x + 1) }
        }
        fn block_on<F: MyFuture>(mut f: F) -> F::Output {
            loop {
                match f.poll() {
                    Poll::Ready(v) => return v,
                    Poll::Pending => { }
                }
            }
        }
        fn main() -> i32 { let f = AddOne { x: 41 }; block_on(f) }
    "#;
    let output = compile(src);
    eprintln!("PROBE-COUNT={}", output.diagnostics.len());
    for d in &output.diagnostics {
        eprintln!("PROBE-DIAG: {} | span={:?}", d.message, d.span);
    }
    // The P5 async-desugar target now compiles cleanly: generic-body
    // monomorphization + associated-type projection are resolved.
    assert!(
        output.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn closure_with_capture_compiles() {
    let src = r#"
        fn main() -> i32 {
            let base = 10;
            let add_base = |n: i32| n + base;
            add_base(5)
        }
    "#;
    let output = compile(src);
    eprintln!("PROBE-COUNT={}", output.diagnostics.len());
    for d in &output.diagnostics {
        eprintln!("PROBE-DIAG: {} | span={:?}", d.message, d.span);
    }
    assert!(
        output.diagnostics.is_empty(),
        "closure-with-capture target should compile cleanly, got: {:?}",
        output.diagnostics
    );
}
