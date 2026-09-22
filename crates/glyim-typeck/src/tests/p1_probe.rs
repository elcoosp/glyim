//! Real-pipeline probes for remaining P1 gaps. Modeled on the proven-working
//! `block_on<F: Future> -> F::Output` pattern in async_await.rs.
use glyim_span::FileId;
use glyim_test::assert_no_errors;
use glyim_test::harness::compiler::{CompileOutput, PipelineCompiler, TestCompiler};
use std::sync::Arc;

use glyim_test::mock::MockCodegen;

fn compile(src: &str) -> CompileOutput {
    let backend = Arc::new(MockCodegen::new());
    let compiler = PipelineCompiler::new(backend);
    compiler.compile(src, std::path::Path::new("test.g"), FileId::from_raw(1), &[])
}

#[test]
fn p1a_concrete_impl_output_projection() {
    // Non-generic impl so the projection's self type is concrete (no inference
    // var). Confirms impl-param substitution normalizes `F::Output` to `i32`.
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
        struct ReadyInt { val: i32 }
        impl Future for ReadyInt {
            type Output = i32;
            fn poll(&mut self) -> Poll<i32> { Poll::Ready(self.val) }
        }
        fn main() -> i32 {
            let r = ReadyInt { val: 7 };
            block_on(r)
        }
    "#;
    let output = compile(src);
    for d in &output.diagnostics {
        eprintln!("DIAG: {:?}", d.message);
    }
    assert_no_errors(&output.diagnostics);
}

#[test]
#[ignore = "KNOWN GAP (p1a): after the generic-impl `collect_generic_params` fix, the \
impl's type param T now scopes into the impl body, but the impl's T index is not \
yet unified with the self-ADT's structural T at the call site (mismatched types: \
T vs i32). Concrete (non-generic) impl projection is fixed and covered above."]
fn p1a_generic_impl_concrete_arg_projection() {
    // Generic impl `impl<T> Future for ReadyT<T>` with a concrete `ReadyT<i32>`
    // argument. Requires impl-param substitution (T := i32) during projection
    // normalization.
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
        struct ReadyT<T> { val: T }
        impl<T> Future for ReadyT<T> {
            type Output = T;
            fn poll(&mut self) -> Poll<T> { Poll::Ready(self.val) }
        }
        fn make() -> ReadyT<i32> { ReadyT { val: 7 } }
        fn main() -> i32 {
            let r = make();
            block_on(r)
        }
    "#;
    let output = compile(src);
    for d in &output.diagnostics {
        eprintln!("DIAG: {:?}", d.message);
    }
    assert_no_errors(&output.diagnostics);
}

#[test]
#[ignore = "KNOWN GAP (p1d/Phase 5): same root as p1a-gen — generic impl<T> does not \
scope its type param T into the impl body, and the impl's T is not unified with \
the self-ADT's structural param (ReadyT<T>'s T). So autoderef through a generic \
smart pointer (`impl<T> Deref for Box<T>`) is not yet wired. Concrete (non-generic) \
Deref impls populate the registry (see deref_impl.rs)."]
fn p1d_deref_autoderef() {
    // A smart-pointer `Box<T>` that derefs to `T`; calling an inherent method on
    // the inner type through the wrapper must autoderef. Verifies Phase 5
    // (Deref autoderef for ADTs) end-to-end via the real pipeline.
    let src = r#"
        struct Box<T> { ptr: T }
        impl<T> Deref for Box<T> {
            type Target = T;
        }
        struct Inner { value: i32 }
        impl Inner {
            fn get(&self) -> i32 { self.value }
        }
        fn main() -> i32 {
            let b = Box { ptr: Inner { value: 9 } };
            b.get()
        }
    "#;
    let output = compile(src);
    for d in &output.diagnostics {
        eprintln!("DIAG: {:?}", d.message);
    }
    assert_no_errors(&output.diagnostics);
}
