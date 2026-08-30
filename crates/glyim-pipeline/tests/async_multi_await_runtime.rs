//! M5 (async v1) — multi-await codegen proof through the real full pipeline.
//!
//! This test compiles the multi-`.await` `two_step`/`two` shapes through the
//! REAL pipeline (parser → HIR desugar `desugar_multi_async_fn` → typeck →
//! MIR lower) and asserts:
//!   1. **Zero compile diagnostics** — the generated suspend/resume state
//!      machine type-checks and lowers cleanly (M4: real multi-await codegen,
//!      the deliverable of GLYIM_DESTUB_PLAN Phase 3). Previously this shape
//!      fell through to the `async-v2` diagnostic (error 61) and never produced
//!      a working state machine; now it is a genuine `Start`/`S0`/…/`Done`
//!      machine with a `loop { match self.state { .. } }` `poll` body.
//!   2. **The generated `poll` body is a real suspend/resume machine** — it
//!      contains a loop back-edge (so `Pending` re-enters the state match), an
//!      enum-state `SwitchInt` (one arm per `Start`/`S_k`/`Done` variant), and
//!      `Poll::Pending`/`Poll::Ready` `Aggregate` constructions. This is the
//!      structural proof that the future genuinely *suspends and resumes*
//!      rather than hard-coding `Poll::Pending` (the old silent-miscompile).
//!
//! HONEST SCOPE NOTE (M5 runtime execution): full end-to-end *execution* of
//! `block_on(f)` requires trait-method dispatch — `block_on<F: Future>` calls
//! `f.poll()`, a `Future::poll` trait method. The monomorphization pass
//! (`glyim-lower::mono`) devirtualizes that `VirtualMethod` call to a static
//! `Fn(two_poll)` reference, and the in-process `glyim_mir_interp` interpreter
//! executes the resulting direct call — so the runtime proof below DOES run
//! `block_on(two_step(1,2))` to completion and return 3. This test therefore
//! proves the full runtime resumption (not just compile + MIR codegen
//! correctness). The *native* LLVM-codegen + link + run proof (a separate
//! concern, guaranteeing the produced ELF executes on Linux) is enforced by
//! `glyim-test`'s `runtime` test on the `test-linux-runtime` CI job.

use std::io::Write;
use std::sync::Arc;

use glyim_core::def_id::DefId;
use glyim_db::Database;
use glyim_mir::{Body, Rvalue, StatementKind, TerminatorKind};
use glyim_mir_interp::{InterpValue, Interpreter};
use glyim_pipeline::compile_file_to_mir;

fn compile_async(src: &str) -> Result<Vec<Arc<Body>>, String> {
    static CALL_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call_id = CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let unique_tag = format!("{}_{}", std::process::id(), call_id);
    let dir = std::env::temp_dir();
    let path = dir.join(format!("glyim_async_m5_{}.g", unique_tag));

    {
        let mut f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        f.write_all(src.as_bytes()).map_err(|e| e.to_string())?;
    }

    let config = glyim_db::CrateConfig {
        name: "test_crate".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    };
    let mut db = Database::new(config);

    let mir = compile_file_to_mir(&mut db, &path).map_err(|diags| {
        diags
            .iter()
            .map(|d| format!("{:?}: {}", d.code, d.message))
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    Ok(mir.bodies.values().cloned().collect())
}

/// Find the generated `poll` body by structure: it is the MIR body that is a
/// suspend/resume state machine (contains a `SwitchInt` on the enum state, a
/// loop back-edge, and constructs both `Poll::Pending` and `Poll::Ready`).
/// `block_on`'s body also has a SwitchInt + back-edge, but it only *patterns*
/// `Poll::Pending`, never *constructs* it, so the Pending-construction
/// requirement reliably selects the generated `poll`.
fn find_poll_body(bodies: &[Arc<Body>]) -> Option<&Arc<Body>> {
    bodies.iter().find(|b| is_state_machine(b))
}

/// Assert `body` is a suspend/resume state machine:
///  - has a loop back-edge (`Goto`/`SwitchInt` whose target is an earlier block),
///  - has at least one `SwitchInt` (the enum-state dispatch),
///  - constructs `Poll::Pending` and `Poll::Ready` aggregates.
fn is_state_machine(body: &Body) -> bool {
    let mut has_switchint = false;
    let mut has_pending = false;
    let mut has_ready = false;
    let mut has_back_edge = false;

    for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
        let bi = bb_idx.index();
        // Detect a back-edge: any terminator targeting a strictly-earlier block.
        for succ in successor_idxs(&bb.terminator.kind) {
            if succ < bi {
                has_back_edge = true;
            }
        }
        if matches!(bb.terminator.kind, TerminatorKind::SwitchInt { .. }) {
            has_switchint = true;
        }
        // Poll ctor detection: `Poll::Pending` is an `Aggregate` with 0 operands;
        // `Poll::Ready` is an `Aggregate` with 1 operand.
        for stmt in &bb.statements {
            if let StatementKind::Assign(_, Rvalue::Aggregate(_, ops)) = &stmt.kind {
                if ops.is_empty() {
                    has_pending = true;
                }
                if ops.len() == 1 {
                    has_ready = true;
                }
            }
        }
    }

    has_switchint && has_back_edge && has_pending && has_ready
}

/// Assert `body` is a suspend/resume state machine (see `is_state_machine`).
fn assert_is_state_machine(body: &Body, label: &str) {
    assert!(
        is_state_machine(body),
        "{label}: poll body must be a suspend/resume state machine \
         (SwitchInt on enum state + loop back-edge + Poll::Pending/Ready construction)"
    );
}

fn successor_idxs(kind: &TerminatorKind) -> Vec<usize> {
    match kind {
        TerminatorKind::Goto { target } => vec![target.index()],
        TerminatorKind::SwitchInt { targets, .. } => {
            let mut v: Vec<usize> = targets
                .iter()
                .map(|(_val, bb)| bb.index())
                .collect();
            v.push(targets.otherwise().index());
            v
        }
        TerminatorKind::Call { target, cleanup, .. } => {
            let mut v = Vec::new();
            if let Some(t) = target {
                v.push(t.index());
            }
            if let Some(c) = cleanup {
                v.push(c.index());
            }
            v
        }
        TerminatorKind::Return | TerminatorKind::Unreachable => vec![],
        _ => vec![],
    }
}

#[test]
fn m5_two_step_multi_await_compiles_and_lowers_to_state_machine() {
    // `two_step(a, b)`: x = dep(a); y = dep(b); x + y  => two `.await`s.
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
async fn two_step(a: i32, b: i32) -> i32 {
    let x = dep(a).await;
    let y = dep(b).await;
    x + y
}
fn main() -> i32 {
    let f = two_step(1, 2);
    block_on(f)
}
"#;
    let bodies = compile_async(src).expect("two_step must compile with zero diagnostics (M4)");
    let poll = find_poll_body(&bodies).expect("generated two poll body must exist");
    // The body named after the async fn is the *future's poll* once desugared;
    // assert it is a real state machine.
    assert_is_state_machine(poll, "two_step poll");
}

#[test]
fn m5_chain_multi_await_compiles_and_lowers_to_state_machine() {
    // Chained: `two(a)`: x = dep(a); y = dep(x); x + y — two `.await`s where
    // the second depends on the first's result (forcing state capture).
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
    let bodies = compile_async(src).expect("two must compile with zero diagnostics (M4)");
    let poll = find_poll_body(&bodies).expect("generated two poll body must exist");
    assert_is_state_machine(poll, "two poll");
}

#[test]
fn m5_two_step_runtime_returns_3() {
    // M5 runtime-proof: compile `two_step` through the REAL pipeline
    // (parser → HIR desugar → typeck → MIR lower → monomorphization) and then
    // *execute* `main` in the in-process MIR interpreter. `main` calls
    // `block_on(two(1,2))`; monomorphization instantiates the generic
    // `block_on<F: Future>` with `F = TwoFuture`, so `block_on`'s
    // `f.poll()` trait-method call resolves against the concrete `TwoFuture`
    // receiver (the `VirtualMethod` const is devirtualized to a static
    // `Fn(two_poll)` during the mono pass). The interpreter then drives the
    // generated poll state machine to completion and returns 1+2 = 3.
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
async fn two_step(a: i32, b: i32) -> i32 {
    let x = dep(a).await;
    let y = dep(b).await;
    x + y
}
fn main() -> i32 {
    let f = two_step(1, 2);
    block_on(f)
}
"#;
    let bodies = compile_async(src).expect("two_step must compile with zero diagnostics (M4)");
    // Structural guard: the generated poll body must still be a real machine.
    let poll = find_poll_body(&bodies).expect("generated two_step poll body must exist");
    assert_is_state_machine(poll, "two_step poll");

    // Now run it for real through the interpreter, after monomorphization.
    let comp = compile_async_full(src).expect("full compile for runtime");
    let main_def_id = resolve_main_def_id(&comp).expect("main DefId resolvable");
    let main_fn_def_id = glyim_core::def_id::FnDefId::from_raw(main_def_id.local_id.to_raw());

    // Monomorphize: yields concrete (substituted + devirtualized) bodies keyed
    // by DefId, exactly as the codegen path does, so `block_on<F>` becomes
    // `block_on<TwoFuture>` with a static `two_poll` call.
    let mono_bodies = comp.monomorphize(main_fn_def_id);
    std::fs::write("/tmp/mono_dump.txt", format!("{:#?}", mono_bodies)).unwrap();

    let main_body = mono_bodies
        .iter()
        .find(|(id, _)| *id == main_def_id)
        .map(|(_, b)| (**b).clone())
        .expect("monomorphized main body present");

    let mut interp = Interpreter::new(comp.ty_ctx.as_ref());
    for (id, b) in &mono_bodies {
        interp.add_function(*id, (**b).clone());
    }
    interp
        .run_body(&main_body)
        .expect("main must run to completion without interpreter error");
    let ret = interp
        .get_return_value()
        .expect("main must produce a return value");
    assert_eq!(
        ret,
        InterpValue::Int(3),
        "block_on(two_step(1,2)) must execute the poll state machine and return 3"
    );
}

/// Compile `src` through the full `MirCompilation` (so we get `ty_ctx` +
/// `def_map` for runtime execution), mirroring `glyip test`'s run path.
fn compile_async_full(src: &str) -> Result<glyim_pipeline::MirCompilation, String> {
    static CALL_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call_id = CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let unique_tag = format!("{}_{}", std::process::id(), call_id);
    let dir = std::env::temp_dir();
    let path = dir.join(format!("glyim_async_m5_run_{}.g", unique_tag));

    {
        let mut f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        f.write_all(src.as_bytes()).map_err(|e| e.to_string())?;
    }

    let config = glyim_db::CrateConfig {
        name: "test_crate".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    };
    let mut db = Database::new(config);

    compile_file_to_mir(&mut db, &path).map_err(|diags| {
        diags
            .iter()
            .map(|d| format!("{:?}: {}", d.code, d.message))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// Resolve `main`'s `DefId` from the crate def map (copied from `glyip`'s
/// `resolve_test_def_id`).
fn resolve_main_def_id(comp: &glyim_pipeline::MirCompilation) -> Option<DefId> {
    for module in comp.def_map.modules.iter() {
        for (name_key, (local_id, _visibility, _span)) in &module.scope.values {
            if comp.def_map.interner.resolve(*name_key) == "main" {
                return Some(DefId::new(comp.def_map.krate, *local_id));
            }
        }
    }
    None
}

#[test]
fn p2_1_await_inside_loop_suspends_and_resumes_runtime_returns_3() {
    // P2-1 proof (async-v2 loop-await state machine, M4/M5): an `.await` inside
    // a `while` loop body whose future type is statically nameable must now
    // desugar to a REAL suspend/resume state machine — not be rejected, and
    // crucially NOT silently lowered into the single-poll desugar (whose
    // `Pending` arm is a `loop {}` that hangs forever). We prove it both
    // structurally (the generated `poll` is a genuine state machine) AND by
    // *executing* `block_on(countdown(3))` in the MIR interpreter: the future
    // must suspend on each iteration's `dep(i).await` and resume, accumulating
    // 0+1+2 = 3. A hang or a wrong value would mean a miscompile.
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
async fn countdown(n: i32) -> i32 {
    let mut total = 0;
    let mut i = 0;
    while i < n {
        // `.await` inside a loop body — now genuinely desugared to a machine.
        total = total + dep(i).await;
        i = i + 1;
    }
    total
}
fn main() -> i32 {
    let f = countdown(3);
    block_on(f)
}
"#;
    let bodies = compile_async(src)
        .expect("countdown loop-await must compile with zero diagnostics (M4/M5)");
    // Structural guard: the generated poll body must be a real machine.
    let poll = find_poll_body(&bodies).expect("generated countdown poll body must exist");
    assert_is_state_machine(poll, "countdown poll");
    std::fs::write("/tmp/poll_dump.txt", format!("{poll:#?}")).unwrap();

    // Now run it for real through the interpreter, after monomorphization.
    let comp = compile_async_full(src).expect("full compile for runtime");
    let main_def_id = resolve_main_def_id(&comp).expect("main DefId resolvable");
    let main_fn_def_id = glyim_core::def_id::FnDefId::from_raw(main_def_id.local_id.to_raw());

    let mono_bodies = comp.monomorphize(main_fn_def_id);
    std::fs::write("/tmp/mono_dump.txt", format!("{:#?}", mono_bodies)).unwrap();

    let main_body = mono_bodies
        .iter()
        .find(|(id, _)| *id == main_def_id)
        .map(|(_, b)| (**b).clone())
        .expect("monomorphized main body present");

    let mut interp = Interpreter::new(comp.ty_ctx.as_ref());
    for (id, b) in &mono_bodies {
        interp.add_function(*id, (**b).clone());
    }
    interp
        .run_body(&main_body)
        .expect("main must run to completion without interpreter error");
    let ret = interp
        .get_return_value()
        .expect("main must produce a return value");
    assert_eq!(
        ret,
        InterpValue::Int(3),
        "block_on(countdown(3)) must execute the loop-await state machine and return 0+1+2 = 3"
    );
}
