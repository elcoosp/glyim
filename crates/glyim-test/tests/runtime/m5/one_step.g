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
// STATUS (2026-09-21): the native async codegen path is CLOSED. The
// `fn_abi_of` panic and the downstream enum `Field`-offset OOB were fixed
// together with a post-inference THIR zonk pass (the LLVM backend no longer
// sees a raw `Infer(Int(_))` from an unsuffixed literal). On the
// `test-linux-runtime` (ubuntu-latest) job the harness compiles with the real
// `LlvmBackend` (feature `real-llvm` via `GLYIM_TEST_REAL_LLVM`), links a
// runnable ELF, and asserts it prints `42`. Cross-compiling the same fixture
// to `x86_64-unknown-linux-gnu` from a non-Linux host also produces a valid,
// linkable ELF object today; only *running* it needs a Linux host (the
// `only-target:` gate handles that).
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
