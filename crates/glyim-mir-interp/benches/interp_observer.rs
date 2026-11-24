//! `InterpObserver` overhead timing harness (M1).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-mir-interp`
//! `run_program` bench and the repo's other `harness = false` benches: only
//! `std::time::Instant` + `std::hint::black_box` (the `glyim-test`
//! dev-dependency provides the blessed `test_ty_ctx` helper, same as the
//! crate's unit tests). The observer seam fires once per basic block
//! entered, so a branchy loop body (`loop_sum(10_000)` — ~40k BB enters)
//! amplifies the per-dispatch cost the seam adds. This bench prints the
//! per-run cost for the three configurations so CI logs can track the
//! seam's overhead against the no-observer baseline. Run with:
//!
//! ```text
//! cargo bench -p glyim-mir-interp --bench interp_observer
//! ```

use glyim_core::{BinOp, CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_mir_interp::{InterpValue, Interpreter, NoopObserver};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Ty, TyKind};
use std::hint::black_box;
use std::time::Instant;

/// A local slot declaration.
fn decl(ty: Ty, mutability: Mutability) -> LocalDecl {
    LocalDecl {
        ty,
        mutability,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn int_const(ty: Ty, v: i128) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Int(v),
        ty,
        span: Span::DUMMY,
    })
}

fn stmt(kind: StatementKind) -> Statement {
    Statement {
        kind,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn block(stmts: Vec<Statement>, terminator: TerminatorKind) -> BasicBlockData {
    BasicBlockData {
        statements: stmts,
        terminator: Terminator {
            kind: terminator,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }
}

/// `fn main() -> i32 { sum += i; i += 1; while i != N }` — the branchy
/// loop shape that maximises basic-block entries (the observer's hot path)
/// per unit of real work. Identical to `run_program`'s `loop_sum_body`
/// shape; duplicated here because benches are standalone binaries.
///
/// bb0: `_2 = 0; _1 = 0; goto head` · bb1 (head): `switch (_2 == N) → exit
/// | body` · bb2 (body): `_1 += _2; _2 += 1; goto head` · bb3 (exit):
/// `_0 = _1; return`.
fn loop_sum_body(owner_raw: u32, n: i128, ty: Ty) -> Body {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(owner_raw),
    ));
    body.locals = IndexVec::from_raw(vec![
        decl(ty, Mutability::Not),
        decl(ty, Mutability::Mut),
        decl(ty, Mutability::Mut),
        decl(ty, Mutability::Mut),
    ]);
    let zero = int_const(ty, 0);
    let one = int_const(ty, 1);
    let sum = || Place::new(LocalIdx::from_raw(1));
    let i = || Place::new(LocalIdx::from_raw(2));
    let tmp = || Place::new(LocalIdx::from_raw(3));
    let ret = || Place::new(LocalIdx::from_raw(0));
    let head = BasicBlockIdx::from_raw(1);
    let body_bb = BasicBlockIdx::from_raw(2);
    let exit_bb = BasicBlockIdx::from_raw(3);
    body.basic_blocks = IndexVec::from_raw(vec![
        block(
            vec![
                stmt(StatementKind::Assign(i(), Rvalue::Use(zero.clone()))),
                stmt(StatementKind::Assign(sum(), Rvalue::Use(zero))),
            ],
            TerminatorKind::Goto { target: head },
        ),
        block(
            vec![stmt(StatementKind::Assign(
                tmp(),
                Rvalue::binary_op(BinOp::Eq, Operand::Copy(i()), int_const(ty, n)),
            ))],
            TerminatorKind::SwitchInt {
                discr: Operand::Copy(tmp()),
                switch_ty: ty,
                targets: SwitchTargets::new(Box::new([(1, exit_bb)]), body_bb),
            },
        ),
        block(
            vec![
                stmt(StatementKind::Assign(
                    sum(),
                    Rvalue::binary_op(BinOp::Add, Operand::Copy(sum()), Operand::Copy(i())),
                )),
                stmt(StatementKind::Assign(
                    i(),
                    Rvalue::binary_op(BinOp::Add, Operand::Copy(i()), one),
                )),
            ],
            TerminatorKind::Goto { target: head },
        ),
        block(
            vec![stmt(StatementKind::Assign(
                ret(),
                Rvalue::Use(Operand::Copy(sum())),
            ))],
            TerminatorKind::Return,
        ),
    ]);
    body
}

/// Run `body` `iter` times, returning mean µs/run. `make_interp` builds a
/// fresh interpreter each iteration so observer state never leaks between
/// runs (the trace collector would otherwise grow unboundedly).
fn main() {
    let mut tcx_mut = test_ty_ctx();
    let i32t = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let tcx = tcx_mut.freeze();

    let n: i128 = 10_000;
    let body = loop_sum_body(0, n, i32t);

    // Correctness probe: the loop must sum to N*(N-1)/2 and the observer
    // must record one event per BB enter (3 statements-block + 1 head
    // re-entry per iteration + the entry/exit blocks).
    {
        let mut interp = Interpreter::new(&tcx).with_trace_collector();
        interp.run_body(&body).unwrap();
        let val = interp
            .get_local_value(LocalIdx::from_raw(0))
            .expect("return place");
        assert_eq!(
            val,
            &InterpValue::Int(n * (n - 1) / 2),
            "loop_sum(10_000) must be exact"
        );
        let events = interp.take_trace().expect("trace collector installed");
        // bb0 (1) + head (N+1: the loop condition re-enters head for i=0..=N)
        // + body (N) + exit (1) = 2N + 3 BbEnter events. The entry body's
        // final `Return` fires `on_call_return`, not `on_bb_enter`.
        let bb_enters = events
            .iter()
            .filter(|e| matches!(e, glyim_mir_interp::TraceEvent::BbEnter { .. }))
            .count();
        assert_eq!(
            bb_enters,
            2 * n as usize + 3,
            "one BbEnter per block entered: bb0 + head*(N+1) + body*N + exit"
        );
    }

    const ITER: usize = 50;

    // (a) baseline — no observer installed (the `Option::is_some` branch
    // always falls through).
    let baseline_us = {
        let t = Instant::now();
        for _ in 0..ITER {
            let mut interp = Interpreter::new(&tcx);
            let _ = black_box(interp.run_body(black_box(&body)));
        }
        t.elapsed().as_micros() as f64 / ITER as f64
    };

    // (b) Noop observer — vtable dispatch per BB enter, no recording.
    let noop_us = {
        let t = Instant::now();
        for _ in 0..ITER {
            let mut interp = Interpreter::new(&tcx).with_observer(Box::new(NoopObserver));
            let _ = black_box(interp.run_body(black_box(&body)));
        }
        t.elapsed().as_micros() as f64 / ITER as f64
    };

    // (c) TraceCollector — vtable + Vec push per BB enter.
    let trace_us = {
        let t = Instant::now();
        for _ in 0..ITER {
            let mut interp = Interpreter::new(&tcx).with_trace_collector();
            let _ = black_box(interp.run_body(black_box(&body)));
        }
        t.elapsed().as_micros() as f64 / ITER as f64
    };

    println!("interp_observer bench (sanity assertions passed)");
    println!("  loop_sum(10_000)  {baseline_us:9.2} µs/run  (no observer — baseline)");
    println!("  loop_sum(10_000)  {noop_us:9.2} µs/run  (NoopObserver — vtable only)");
    println!("  loop_sum(10_000)  {trace_us:9.2} µs/run  (TraceCollector — vtable + Vec push)");
}
