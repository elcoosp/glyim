// M5 (async v1) — single-await end-to-end runtime proof.
//
// Compiles the supported single-await shape through the real pipeline and runs
// it: `add_one(41)` must return 42 via block_on. This fixture is the verified
// supported subset of M5 (the desugar type-checks with zero diagnostics via the
// `PipelineCompiler`; see `nested_async_single_await_compiles` / `desugar_async_fn_compiles`).
//
// GATED TO LINUX: the pipeline links a native x86_64-unknown-linux-gnu binary,
// so this only executes on that target (macOS/Windows runners Ignore it).
//
// STATUS (2026-08-28): the test harness now uses the real `LlvmBackend`
// (feature `real-llvm`, enabled by `GLYIM_TEST_REAL_LLVM` on the Linux job) and
// this fixture is run for real, asserting it prints `42`. The NATIVE async
// codegen path is CURRENTLY BROKEN (tracked gap): `glyim-codegen-llvm` panics
// in `fn_abi_of` (`lower.rs`) for the monomorphized `block_on<F>` / `poll`
// types, so the Linux job currently FAILS here until that gap is fixed. The
// MIR-interpreter proof (`glyim-pipeline::async_multi_await_runtime`) still
// verifies this shape end-to-end today.
// test-mode: run-pass
// only-target: x86_64-unknown-linux-gnu
// check-stdout: 42

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
