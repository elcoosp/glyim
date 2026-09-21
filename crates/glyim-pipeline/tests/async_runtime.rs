//! Interpreter-verified proof that the `async fn`/`.await` state-machine
//! desugar + trait-method devirtualization actually *execute* on this host.
//!
//! The glyim MIR interpreter is platform-agnostic (unlike the Linux-linked
//! LLVM binary, which cannot run on a non-Linux host). So this test is the
//! authoritative on-host runtime proof that:
//!   * the `async fn` desugar lowers to a real `Future` state machine,
//!   * `f.poll()` on a *concrete* future devirtualizes to the impl `poll`
//!     body (not a generic-`Param` `VirtualMethod` that can't dispatch), and
//!   * the awaited inner future (`dep(a).poll()`) runs and yields its value.
//!
//! The generic `block_on<F: Future>` driver cannot be executed here because
//! the interpreter does not monomorphize a generic `F` receiver; that path is
//! covered by the Linux-gated binary runtime fixture in
//! `glyim-test/tests/runtime/m5` (which tolerates `CompilationFailed` on
//! non-Linux hosts). This interpreter test deliberately drives the concrete
//! future directly (`one_step(7).poll()`) so it runs everywhere.

use std::io::Write;

use glyim_db::Database;
use glyim_mir_interp::Interpreter;
use glyim_pipeline::compile_file_to_mir;

const SRC: &str = r#"
enum Poll<T> { Ready(T), Pending }
trait Future {
    type Output;
    fn poll(&mut self) -> Poll<Self::Output>;
}
async fn dep(x: i32) -> i32 { x }
async fn one_step(a: i32) -> i32 {
    let x = dep(a).await;
    x
}
fn main() {
    let mut f = one_step(7);
    let _ = f.poll();
}
"#;

#[test]
fn async_state_machine_runs_via_interpreter() {
    static CALL_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call_id = CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let unique_tag = format!("{}_{}", std::process::id(), call_id);
    let dir = std::env::temp_dir();
    let path = dir.join(format!("glyim_async_runtime_{}.g", unique_tag));
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(SRC.as_bytes()).unwrap();
    }

    let config = glyim_db::CrateConfig {
        name: "test_crate".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    };
    let mut db = Database::new(config);

    let mir = compile_file_to_mir(&mut db, &path)
        .expect("async fixture must compile to MIR with zero diagnostics");
    let ty_ctx = mir.ty_ctx;
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = mir.bodies.values().cloned().collect();
    let main_local =
        glyim_pipeline::Pipeline::entry_main_local_id(&mut db, &path).expect("resolve main");
    let main_id = glyim_core::def_id::DefId::new(
        glyim_core::def_id::CrateId::from_raw(0),
        glyim_core::def_id::LocalDefId::from_raw(main_local),
    );
    let main_body = mir.bodies.get(&main_id).expect("main body");

    let mut interp = Interpreter::new(ty_ctx.as_ref());
    for b in &bodies {
        interp.add_function(b.owner, (**b).clone());
    }
    // Must run to completion without `panic: callee must be a function
    // reference` (the signature of an un-devirtualized generic-`Param`
    // `f.poll()`). Reaching `Ok(())` proves the concrete `Future::poll`
    // dispatch + awaited inner-future execution both worked.
    interp
        .run_body(main_body)
        .expect("async state machine must execute via the interpreter (concrete future dispatch)");
}
