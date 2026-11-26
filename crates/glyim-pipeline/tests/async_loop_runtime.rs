//! GAP-0006 `loop`-shape closure — `.await` inside an unbounded `loop` body.
//!
//! Mirrors `async_for_loop_runtime.rs`: compiles the `loop`-body await shape
//! through the REAL pipeline (parser → HIR desugar → typeck → MIR lower) and
//! asserts
//!   1. **Zero compile diagnostics** — the witness shape that was a
//!      deliberate hard error (T0060 / the `loop` half of GAP-0006) now
//!      desugars to a `Start`/`S0`/`Done` machine, exactly like the `while`
//!      and `for`-over-range shapes (the desugar normalizes `loop { .. }`
//!      into the `while true { .. }` shape the machine already re-drives).
//!   2. **The generated `poll` body is a real suspend/resume machine** —
//!      loop back-edge, enum-state `SwitchInt`, `Poll::Pending`/`Ready`
//!      aggregates (the structural suspension proof).
//!   3. **Runtime execution returns the right value** — `block_on` drives
//!      the machine in the MIR interpreter; `break` / `break v` /
//!      `continue` inside the loop body translate to the machine's own
//!      `Done`/loop-step transitions, so a `loop` await future resolves
//!      with the value the loop exited on (a hang or a wrong value would
//!      mean a miscompile).
//!
//! Honest scope note: the remaining GAP-0006 shapes — a `for` over a
//! non-range iterable and destructuring loop patterns — stay on the loud
//! T0060 diagnostic; the compile-fail witness in `glyim-test` pins them.

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
    let path = dir.join(format!("glyim_async_loop_{unique_tag}.g"));

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
    let path = dir.join(format!("glyim_async_loop_run_{unique_tag}.g"));

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
fn gap_0006_loop_body_await_compiles_and_lowers_to_state_machine() {
    // The retired `loop`-shape witness: a single `.await` inside a `loop`
    // body must now desugar (previously the loud T0060 hard error). The
    // desugar normalizes `loop { .. }` into the `while true { .. }` shape
    // the coroutine machine already re-drives.
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
    let mut i = 0;
    loop {
        acc = acc + dep(i).await;
        i = i + 1;
        if i >= 3 {
            break acc;
        }
    }
}
fn main() -> i32 {
    let f = spin();
    block_on(f)
}
";
    let bodies = compile_async(src)
        .expect("loop-body await must compile with zero diagnostics (GAP-0006 loop shape closed)");
    let poll = find_poll_body(&bodies).expect("generated spin poll body must exist");
    assert_is_state_machine(poll, "spin poll (loop-body shape)");
}

#[test]
fn gap_0006_loop_body_await_break_value_resolves_the_future() {
    // `break acc` inside the loop-await body must become the machine's
    // `Done { result: acc }` transition, so the future resolves with the
    // value the loop exited on: acc accumulates 0+0+1+2 = 3 and `break acc`
    // yields 3.
    let src = format!(
        r"
{BLOCK_ON_AND_HELPERS}
async fn spin() -> i32 {{
    let mut acc = 0;
    let mut i = 0;
    loop {{
        acc = acc + dep(i).await;
        i = i + 1;
        if i >= 3 {{
            break acc;
        }}
    }}
}}
fn main() -> i32 {{
    let f = spin();
    block_on(f)
}}
"
    );
    let ret = run_main_returns(&src);
    assert_eq!(
        ret,
        InterpValue::Int(3),
        "`break acc` must resolve the future with the value the loop exited on (3)"
    );
}

#[test]
fn gap_0006_loop_body_plain_break_uses_the_tail_local() {
    // A value-less `break` becomes `Done { result: <tail local> }`: the
    // accumulator lives in the state enum across `Poll::Pending`, so the
    // future resolves with the tail's value at the break point (0+0+1+2 = 3,
    // not the pre-loop 0).
    let src = format!(
        r"
{BLOCK_ON_AND_HELPERS}
async fn spin() -> i32 {{
    let mut acc = 0;
    let mut i = 0;
    loop {{
        acc = acc + dep(i).await;
        i = i + 1;
        if i >= 3 {{
            break;
        }}
    }}
    acc
}}
fn main() -> i32 {{
    let f = spin();
    block_on(f)
}}
"
    );
    let ret = run_main_returns(&src);
    assert_eq!(
        ret,
        InterpValue::Int(3),
        "plain break must resolve with the tail local's value at the break point"
    );
}

#[test]
fn gap_0006_loop_body_continue_starts_next_iteration() {
    // `continue` inside the loop-await body must become the loop step
    // (the next-iteration re-drive), skipping the rest of the body for
    // that iteration — never a re-entry into the dispatch loop with the
    // same `S0` state (which would re-poll the same future forever).
    let src = format!(
        r"
{BLOCK_ON_AND_HELPERS}
async fn spin() -> i32 {{
    let mut acc = 0;
    let mut i = 0;
    let mut extra = 0;
    loop {{
        if i >= 4 {{
            break acc * 100 + extra;
        }}
        acc = acc + dep(i).await;
        i = i + 1;
        if i % 2 == 0 {{
            continue;
        }}
        extra = extra + 10;
    }}
}}
fn main() -> i32 {{
    let f = spin();
    block_on(f)
}}
"
    );
    // i=0: acc+=0, extra+=10; i=1: acc+=1, continue; i=2: acc+=2, extra+=10;
    // i=3: acc+=3, continue; i=4: break -> acc = 6, extra = 20 -> 620.
    let ret = run_main_returns(&src);
    assert_eq!(
        ret,
        InterpValue::Int(620),
        "loop-shape continue must skip the post-continue statements and re-drive the step"
    );
}

#[test]
fn gap_0006_loop_body_if_guard_is_preserved() {
    // The await sits inside an `if` guard inside the `loop`: the machine
    // must re-run the guard, not flatten it. Sum over even i only:
    // 0 + 2 = 2, then break at i = 4.
    let src = format!(
        r"
{BLOCK_ON_AND_HELPERS}
async fn spin() -> i32 {{
    let mut acc = 0;
    let mut i = 0;
    loop {{
        if i >= 4 {{
            break acc;
        }}
        if i % 2 == 0 {{
            acc = acc + dep(i).await;
        }}
        i = i + 1;
    }}
}}
fn main() -> i32 {{
    let f = spin();
    block_on(f)
}}
"
    );
    let ret = run_main_returns(&src);
    assert_eq!(
        ret,
        InterpValue::Int(2),
        "the if guard around the await must be preserved: only even i contribute (0+2 = 2)"
    );
}

#[test]
fn gap_0006_loop_await_nested_in_while_still_rejected_shapes_stay_diagnosed() {
    // Scope guard for the closed shape: a `for` over a NON-range iterable
    // keeps the loud T0060 diagnostic (the remaining GAP-0006 half), never
    // a silent misdesugar.
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
