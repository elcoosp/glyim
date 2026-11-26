//! GAP-0001 closure — `.await` inside a `for` loop over a range.
//!
//! Mirrors `async_multi_await_runtime.rs`: compiles the `for`-loop await
//! shape through the REAL pipeline (parser → HIR desugar → typeck → MIR
//! lower) and asserts
//!   1. **Zero compile diagnostics** — the witness shape that was a
//!      deliberate hard error (T0060 / GAP-0001) now desugars to a
//!      `Start`/`S0`/`Done` machine whose loop step re-drives the in-loop
//!      future, exactly like the `while` shape.
//!   2. **The generated `poll` body is a real suspend/resume machine** —
//!      loop back-edge, enum-state `SwitchInt`, `Poll::Pending`/`Ready`
//!      aggregates (the structural suspension proof).
//!   3. **Runtime execution returns the accumulated sum** — `block_on`
//!      drives the machine in the MIR interpreter; the loop variable and
//!      accumulator live in the state enum, so resumption accumulates
//!      0+1+2 = 3 (a hang or a wrong value would mean a miscompile).
//!
//! Honest scope note: the `dep(i)` futures used here resolve on their
//! first poll, so the *runtime* path exercises the loop-step dispatch and
//! the Done transition; the actual cross-`Pending` suspension is pinned
//! structurally (the `S0` arm stores the iterator field + in-flight
//! future in the state enum, and the MIR constructs `Poll::Pending`).
//! The HIR-level tests in `glyim-hir/src/tests/async_desugar.rs` pin the
//! state-carrying field; a future that pends mid-loop is the natural
//! next runtime fixture (not possible through `async fn` today, whose
//! single-poll desugar resolves on the first poll).

use std::io::Write;
use std::sync::Arc;

use glyim_core::def_id::DefId;
use glyim_db::Database;
use glyim_mir::{Body, Rvalue, StatementKind, TerminatorKind};
use glyim_mir_interp::{InterpValue, Interpreter};
use glyim_pipeline::compile_file_to_mir;

const BLOCK_ON_AND_HELPERS: &str = "
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
";

fn compile_async(src: &str) -> Result<Vec<Arc<Body>>, String> {
    static CALL_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call_id = CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let unique_tag = format!("{}_{}", std::process::id(), call_id);
    let dir = std::env::temp_dir();
    let path = dir.join(format!("glyim_async_for_loop_{unique_tag}.g"));

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

    let mir = compile_file_to_mir(&mut db, &path, 2).map_err(|diags| {
        diags
            .iter()
            .map(|d| format!("{:?}: {}", d.code, d.message))
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    Ok(mir.bodies.values().cloned().collect())
}

/// Compile `src` through the full `MirCompilation` (so we get `ty_ctx` +
/// `def_map` for runtime execution), mirroring `glyip test`'s run path.
fn compile_async_full(src: &str) -> Result<glyim_pipeline::MirCompilation, String> {
    static CALL_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call_id = CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let unique_tag = format!("{}_{}", std::process::id(), call_id);
    let dir = std::env::temp_dir();
    let path = dir.join(format!("glyim_async_for_loop_run_{unique_tag}.g"));

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

    compile_file_to_mir(&mut db, &path, 2).map_err(|diags| {
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

/// Find the generated `poll` body by structure (see `is_state_machine`).
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
        for succ in successor_idxs(&bb.terminator.kind) {
            if succ < bi {
                has_back_edge = true;
            }
        }
        if matches!(bb.terminator.kind, TerminatorKind::SwitchInt { .. }) {
            has_switchint = true;
        }
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
            let mut v: Vec<usize> = targets.iter().map(|(_val, bb)| bb.index()).collect();
            v.push(targets.otherwise().index());
            v
        }
        TerminatorKind::Call {
            target, cleanup, ..
        } => {
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

/// Compile + drive `main` to completion in the interpreter, returning
/// `main`'s return value (mirrors `m5_two_step_runtime_returns_3`).
fn run_main_returns(src: &str) -> InterpValue {
    let comp = compile_async_full(src).expect("full compile for runtime");
    let main_def_id = resolve_main_def_id(&comp).expect("main DefId resolvable");
    let main_fn_def_id = glyim_core::def_id::FnDefId::from_usize(main_def_id.local_id.index());

    let mono_bodies = comp.monomorphize(main_fn_def_id);
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
    interp
        .get_return_value()
        .expect("main must produce a return value")
}

#[test]
fn gap_0001_for_loop_await_compiles_and_lowers_to_state_machine() {
    // The retired GAP-0001 witness shape: a single `.await` inside a `for`
    // body over a half-open range must now desugar (previously the loud
    // T0060 hard error).
    let src = r"
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
async fn spin() -> i32 {
    let mut acc = 0;
    for i in 0..3 {
        acc = acc + dep(i).await;
    }
    acc
}
fn main() -> i32 {
    let f = spin();
    block_on(f)
}
";
    let bodies = compile_async(src)
        .expect("for-loop await must compile with zero diagnostics (GAP-0001 closed)");
    let poll = find_poll_body(&bodies).expect("generated spin poll body must exist");
    assert_is_state_machine(poll, "spin poll (for-loop shape)");
}

#[test]
fn gap_0001_for_loop_await_inclusive_range_compiles() {
    // The inclusive form (`0..=2`) desugars the same way (loop step
    // compares with `<=`).
    let src = r"
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
async fn spin_inclusive() -> i32 {
    let mut acc = 0;
    for i in 0..=2 {
        acc = acc + dep(i).await;
    }
    acc
}
fn main() -> i32 {
    let f = spin_inclusive();
    block_on(f)
}
";
    let bodies = compile_async(src).expect("inclusive for-loop await must compile");
    let poll = find_poll_body(&bodies).expect("generated spin_inclusive poll body must exist");
    assert_is_state_machine(poll, "spin_inclusive poll (inclusive range)");
}

#[test]
fn gap_0001_for_loop_await_runtime_returns_3() {
    // Runtime proof: block_on(spin()) accumulates 0+1+2 = 3 through the
    // for-loop state machine.
    let src = format!(
        r"
{BLOCK_ON_AND_HELPERS}
async fn spin() -> i32 {{
    let mut acc = 0;
    for i in 0..3 {{
        acc = acc + dep(i).await;
    }}
    acc
}}
fn main() -> i32 {{
    let f = spin();
    block_on(f)
}}
"
    );
    let bodies = compile_async(&src).expect("for-loop await runtime fixture must compile");
    let poll = find_poll_body(&bodies).expect("generated spin poll body must exist");
    assert_is_state_machine(poll, "spin poll");

    let ret = run_main_returns(&src);
    assert_eq!(
        ret,
        InterpValue::Int(3),
        "block_on(spin()) must execute the for-loop poll state machine and return 0+1+2 = 3"
    );
}

#[test]
fn gap_0001_for_loop_await_runtime_inclusive_range_returns_3() {
    // Inclusive range `0..=2` iterates 0,1,2 — the same sum, pinning the
    // `<=` loop-step comparison end to end.
    let src = format!(
        r"
{BLOCK_ON_AND_HELPERS}
async fn spin_inclusive() -> i32 {{
    let mut acc = 0;
    for i in 0..=2 {{
        acc = acc + dep(i).await;
    }}
    acc
}}
fn main() -> i32 {{
    let f = spin_inclusive();
    block_on(f)
}}
"
    );
    let bodies = compile_async(&src).expect("inclusive for-loop runtime fixture must compile");
    let poll = find_poll_body(&bodies).expect("generated spin_inclusive poll body must exist");
    assert_is_state_machine(poll, "spin_inclusive poll");

    let ret = run_main_returns(&src);
    assert_eq!(
        ret,
        InterpValue::Int(3),
        "block_on(spin_inclusive()) must iterate 0..=2 and return 3"
    );
}

#[test]
fn gap_0006_for_loop_over_nonrange_iterable_is_still_diagnosed() {
    // The remaining async-v2 shape (new GAP-0006, unwitnessed here — the
    // compile-fail witness lives in glyim-test): awaiting inside a `for`
    // loop over a non-range iterable must still produce the loud T0060
    // diagnostic, never a silent misdesugar.
    let src = r"
enum Poll<T> { Ready(T), Pending }
trait Future {
    type Output;
    fn poll(&mut self) -> Poll<Self::Output>;
}
async fn dep(x: i32) -> i32 { x }
async fn drain(it: Vec<i32>) -> i32 {
    let mut acc = 0;
    for v in it {
        acc = acc + dep(v).await;
    }
    acc
}
fn main() -> i32 { 0 }
";
    let err =
        compile_async(src).expect_err("for-over-non-range await must still be rejected (GAP-0006)");
    assert!(
        err.contains("inside a loop body is not yet supported"),
        "diagnostic must be the async-v2 T0060 hard error; got: {err}"
    );
}

// --- M2: control-flow soundness inside a loop-await body ---
//
// The loop-await machine re-drives the WHOLE loop body (with the await
// substituted by the polled value) in the `S0` Ready arm. Statements that
// jump — `break`, `continue` — and the If/While/Loop wrappers that carry
// them must survive that copy with their original targets: a `break` must
// become the `Done { result }` transition, a `continue` must become the
// loop step (the next-iteration re-drive), and a guarded statement must
// keep its guard. Copying them verbatim would jump out of the dispatch
// loop instead (a miscompile: wrong value, or an unbounded re-poll loop
// that only the interpreter's step limit catches).

#[test]
fn m2_loop_await_body_if_guard_is_preserved() {
    // The await sits inside an `if` guard: the machine must re-run the
    // guard, not flatten it. Sum over odd i only: 1 + 3 = 4.
    let src = format!(
        r"
{BLOCK_ON_AND_HELPERS}
async fn f(n: i32) -> i32 {{
    let mut acc = 0;
    let mut i = 0;
    while i < n {{
        if i % 2 == 1 {{
            acc = acc + dep(i).await;
        }}
        i = i + 1;
    }}
    acc
}}
fn main() -> i32 {{
    let f = f(4);
    block_on(f)
}}
"
    );
    let ret = run_main_returns(&src);
    assert_eq!(
        ret,
        InterpValue::Int(4),
        "the if guard around the await must be preserved: only odd i contribute (1+3 = 4)"
    );
}

#[test]
fn m2_loop_await_body_continue_starts_next_iteration() {
    // `continue` inside the loop-await body must skip the rest of the
    // body and start the next iteration (re-checking the loop condition),
    // never re-enter the dispatch loop with the same state. In the `for`
    // shape the translated continue must also re-run the normalization's
    // iterator increment, or the loop variable would never advance.
    let src = format!(
        r"
{BLOCK_ON_AND_HELPERS}
async fn f(n: i32) -> i32 {{
    let mut acc = 0;
    for i in 0..n {{
        if i % 2 == 0 {{
            continue;
        }}
        acc = acc + dep(i).await;
    }}
    acc
}}
fn main() -> i32 {{
    let f = f(4);
    block_on(f)
}}
"
    );
    let ret = run_main_returns(&src);
    assert_eq!(
        ret,
        InterpValue::Int(4),
        "continue must skip even i, advance the iterator, and re-check: 1 + 3 = 4"
    );
}

#[test]
fn m2_while_await_body_continue_starts_next_iteration() {
    // Same contract over the native `while` shape, in the desugar's
    // supported body shape (await first, pure pre-await segment — the
    // machine re-drives the body copy once per poll, so a `continue` is
    // sound exactly when nothing before the await mutates state): the
    // translated continue jumps to the loop step, skipping the rest of
    // the body for that iteration.
    let src = format!(
        r"
{BLOCK_ON_AND_HELPERS}
async fn f(n: i32) -> i32 {{
    let mut acc = 0;
    let mut extra = 0;
    let mut i = 0;
    while i < n {{
        acc = acc + dep(i).await;
        i = i + 1;
        if i % 2 == 0 {{
            continue;
        }}
        extra = extra + 10;
    }}
    acc * 100 + extra
}}
fn main() -> i32 {{
    let f = f(3);
    block_on(f)
}}
"
    );
    // i=0: acc+=0, extra+=10; i=1: acc+=1, continue; i=2: acc+=2, extra+=10
    // -> acc = 3, extra = 20 -> 320. The continue must skip `extra` on
    // even i and re-drive the loop step with the next iteration's future.
    let ret = run_main_returns(&src);
    assert_eq!(
        ret,
        InterpValue::Int(320),
        "while-shape continue must skip the post-continue statements and re-drive the step"
    );
}

#[test]
fn m2_loop_await_body_break_exits_early() {
    // `break` inside the loop-await body must become the machine's
    // `Done {{ result: <tail local> }}` transition — not a jump out of
    // the poll dispatch loop (which fails to type-check) — so the future
    // resolves early with the tail value the loop exited on.
    let src = format!(
        r"
{BLOCK_ON_AND_HELPERS}
async fn f(n: i32) -> i32 {{
    let mut acc = 0;
    let mut i = 0;
    while i < 100 {{
        acc = acc + dep(i).await;
        i = i + 1;
        if acc >= n {{
            break;
        }}
    }}
    acc
}}
fn main() -> i32 {{
    let f = f(6);
    block_on(f)
}}
"
    );
    let ret = run_main_returns(&src);
    assert_eq!(
        ret,
        InterpValue::Int(6),
        "break must exit the loop-await machine early with the accumulated 1+2+3 = 6"
    );
}
