//! `run_program` process-contract timing harness (M2.4).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-pipeline`
//! `phase_prof` bench and the `glyim-test` `annotations`/`evidence` benches:
//! only `std::time::Instant` + `std::hint::black_box` (the `glyim-test`
//! dev-dependency provides the blessed `test_ty_ctx` helper, same as the
//! crate's unit tests). `--emit=run` and the harness's `InterpRunner` both
//! pay this path once per program run, so its per-call overhead
//! (function-table build + entry resolution + entry-body interpretation)
//! must stay small against a real compile. This bench prints the per-run
//! cost so CI logs can track headroom. Run with:
//!
//! ```text
//! cargo bench -p glyim-mir-interp --bench run_program
//! ```

use glyim_core::{BinOp, CrateId, DefId, FnDefId, IndexVec, IntTy, LocalDefId, Mutability, UintTy};
use glyim_mir::*;
use glyim_mir_interp::{RunKind, run_program};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Substitution, Ty, TyCtxMut, TyKind};
use std::hint::black_box;
use std::sync::Arc;
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

fn i32_ty(tcx: &mut TyCtxMut) -> Ty {
    tcx.mk_ty(TyKind::Int(IntTy::I32))
}

fn usize_ty(tcx: &mut TyCtxMut) -> Ty {
    tcx.mk_ty(TyKind::Uint(UintTy::Usize))
}

/// `fn main() -> i32`: a `sum += i` loop over `[0, N)` — the shape every
/// interpreted run-pass fixture spends its time in (branchy control flow +
/// scalar arithmetic through the interpreter loop).
///
/// bb0: `_2 = 0; _1 = 0; goto head` · bb1 (head): `switch (_2 == N) → exit
/// | body` · bb2 (body): `_1 += _2; _2 += 1; goto head` · bb3 (exit):
/// `_0 = _1; return`.
fn loop_sum_body(owner_raw: u32, n: i128, ty: Ty) -> Body {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(owner_raw),
    ));
    // _0 = return place, _1 = sum, _2 = i, _3 = comparison temp.
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
                Rvalue::BinaryOp(BinOp::Eq, Box::new((Operand::Copy(i()), int_const(ty, n)))),
            ))],
            TerminatorKind::SwitchInt {
                discr: Operand::Copy(tmp()),
                switch_ty: ty,
                // The discr is the `i == N` comparison result (0/1); a `1`
                // routes to the exit block, everything else takes the body.
                targets: SwitchTargets::new(Box::new([(1, exit_bb)]), body_bb),
            },
        ),
        block(
            vec![
                stmt(StatementKind::Assign(
                    sum(),
                    Rvalue::BinaryOp(
                        BinOp::Add,
                        Box::new((Operand::Copy(sum()), Operand::Copy(i()))),
                    ),
                )),
                stmt(StatementKind::Assign(
                    i(),
                    Rvalue::BinaryOp(BinOp::Add, Box::new((Operand::Copy(i()), one))),
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

/// `fn fib(n: i32) -> i32` at the MIR level — the recursive shape that
/// exercises `run_program`'s entry resolution and the interpreter's
/// cross-frame call stack (1 973 calls for `fib(15)`).
///
/// bb0: `switch (n < 2) → then | else` · bb1 (then): `_0 = n; return` ·
/// bb2 (else): `_3 = n - 1; _5 = fib(_3) → bb3` · bb3: `_4 = n - 2;
/// _6 = fib(_4) → bb4` · bb4 (join): `_0 = _5 + _6; return`.
fn fib_body(owner_raw: u32, ty: Ty) -> Body {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(owner_raw),
    ));
    // _0 return, _1 n, _2 cmp, _3/_4 argument temps, _5/_6 call results.
    body.locals = IndexVec::from_raw(vec![
        decl(ty, Mutability::Not),
        decl(ty, Mutability::Mut),
        decl(ty, Mutability::Mut),
        decl(ty, Mutability::Mut),
        decl(ty, Mutability::Mut),
        decl(ty, Mutability::Mut),
        decl(ty, Mutability::Mut),
    ]);
    let n = || Place::new(LocalIdx::from_raw(1));
    let tmp = |raw: u32| Place::new(LocalIdx::from_raw(raw));
    let ret = || Place::new(LocalIdx::from_raw(0));
    let call_fib = |arg: Operand, dest: u32, target: BasicBlockIdx| TerminatorKind::Call {
        func: Operand::Constant(MirConst {
            kind: MirConstKind::Fn(FnDefId::from_raw(owner_raw), Substitution::empty()),
            ty,
            span: Span::DUMMY,
        }),
        args: vec![arg],
        destination: Place::new(LocalIdx::from_raw(dest)),
        target: Some(target),
        cleanup: None,
    };
    let then_bb = BasicBlockIdx::from_raw(1);
    let else_bb = BasicBlockIdx::from_raw(2);
    let call1_bb = BasicBlockIdx::from_raw(3);
    let call2_bb = BasicBlockIdx::from_raw(4);
    body.basic_blocks = IndexVec::from_raw(vec![
        block(
            vec![stmt(StatementKind::Assign(
                tmp(2),
                Rvalue::BinaryOp(BinOp::Lt, Box::new((Operand::Copy(n()), int_const(ty, 2)))),
            ))],
            TerminatorKind::SwitchInt {
                discr: Operand::Copy(tmp(2)),
                switch_ty: ty,
                targets: SwitchTargets::new(Box::new([(1, then_bb)]), else_bb),
            },
        ),
        block(
            vec![stmt(StatementKind::Assign(
                ret(),
                Rvalue::Use(Operand::Copy(n())),
            ))],
            TerminatorKind::Return,
        ),
        block(
            vec![stmt(StatementKind::Assign(
                tmp(3),
                Rvalue::BinaryOp(BinOp::Sub, Box::new((Operand::Copy(n()), int_const(ty, 1)))),
            ))],
            call_fib(Operand::Copy(tmp(3)), 5, call1_bb),
        ),
        // Second call writes its result to _6 (not the shared _5), so the
        // join can add the two recursive results without an extra copy.
        block(
            vec![stmt(StatementKind::Assign(
                tmp(4),
                Rvalue::BinaryOp(BinOp::Sub, Box::new((Operand::Copy(n()), int_const(ty, 2)))),
            ))],
            call_fib(Operand::Copy(tmp(4)), 6, call2_bb),
        ),
        block(
            vec![stmt(StatementKind::Assign(
                ret(),
                Rvalue::BinaryOp(
                    BinOp::Add,
                    Box::new((Operand::Copy(tmp(5)), Operand::Copy(tmp(6)))),
                ),
            ))],
            TerminatorKind::Return,
        ),
    ]);
    body
}

/// `fn main() -> i32 { fib(15) }` — the entry body for the recursion
/// shape. The interpreter binds call arguments into the callee's locals
/// `[1..=arg_count]`, so `fib` must be *called* (never run as the entry
/// body, which receives no arguments).
fn main_calls_fib_body(owner_raw: u32, callee_owner_raw: u32, ty: Ty) -> Body {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(owner_raw),
    ));
    body.locals = IndexVec::from_raw(vec![decl(ty, Mutability::Not)]);
    body.basic_blocks = IndexVec::from_raw(vec![
        block(
            vec![],
            TerminatorKind::Call {
                func: Operand::Constant(MirConst {
                    kind: MirConstKind::Fn(
                        FnDefId::from_raw(callee_owner_raw),
                        Substitution::empty(),
                    ),
                    ty,
                    span: Span::DUMMY,
                }),
                args: vec![int_const(ty, 15)],
                destination: Place::new(LocalIdx::from_raw(0)),
                target: Some(BasicBlockIdx::from_raw(1)),
                cleanup: None,
            },
        ),
        block(vec![], TerminatorKind::Return),
    ]);
    body
}

fn main() {
    let mut tcx_mut = test_ty_ctx();
    let i32t = i32_ty(&mut tcx_mut);
    let _usize_t = usize_ty(&mut tcx_mut);
    let tcx = tcx_mut.freeze();

    // Shape 1: loop program — 10k iterations of scalar arithmetic.
    let loop_n: i128 = 10_000;
    let loop_main = Arc::new(loop_sum_body(0, loop_n, i32t));
    let loop_bodies = vec![Arc::clone(&loop_main)];

    // Shape 2: recursion — fib(15) through the cross-frame call stack,
    // entered from a `main` that calls it (an entry body receives no
    // arguments; only called bodies get theirs bound).
    let fib_main = Arc::new(fib_body(1, i32t));
    let fib_entry = Arc::new(main_calls_fib_body(0, 1, i32t));
    let fib_bodies = vec![Arc::clone(&fib_entry), Arc::clone(&fib_main)];

    // Shape 3: function-table build cost — the loop program plus 199
    // dummy bodies `run_program` must register before executing entry 200.
    let mut many_bodies: Vec<Arc<Body>> = Vec::with_capacity(200);
    for raw in 1..=199u32 {
        many_bodies.push(Arc::new(loop_sum_body(raw, 0, i32t)));
    }
    many_bodies.push(Arc::new(loop_sum_body(200, 0, i32t)));

    const ITER: usize = 200;

    // Correctness probe first: a wrong entry resolution or exit mask must
    // be loud here, not silently benched.
    let outcome = run_program(&loop_bodies, &tcx, None);
    assert_eq!(
        outcome.kind,
        RunKind::Completed,
        "loop bench body must run clean: stderr={:?}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert_eq!(
        outcome.exit_code,
        ((loop_n * (loop_n - 1) / 2) & 0xff) as i32,
        "loop sum must be exact"
    );
    let fib = run_program(&fib_bodies, &tcx, None);
    assert_eq!(
        fib.kind,
        RunKind::Completed,
        "fib bench body must run clean: stderr={:?}",
        String::from_utf8_lossy(&fib.stderr)
    );
    assert_eq!(fib.exit_code, 0x262 & 0xff, "fib(15) == 610 (0x262)");
    let many = run_program(&many_bodies, &tcx, Some(200));
    assert_eq!(
        many.kind,
        RunKind::Completed,
        "table bench body must run clean: stderr={:?}",
        String::from_utf8_lossy(&many.stderr)
    );

    let t = Instant::now();
    for _ in 0..ITER {
        let outcome = black_box(run_program(black_box(&loop_bodies), &tcx, None));
        black_box(outcome.exit_code);
    }
    let loop_us = t.elapsed().as_micros() as f64 / ITER as f64;

    let t = Instant::now();
    for _ in 0..ITER {
        let outcome = black_box(run_program(black_box(&fib_bodies), &tcx, None));
        black_box(outcome.exit_code);
    }
    let fib_us = t.elapsed().as_micros() as f64 / ITER as f64;

    let t = Instant::now();
    for _ in 0..ITER {
        let outcome = black_box(run_program(black_box(&many_bodies), &tcx, Some(200)));
        black_box(outcome.exit_code);
    }
    let many_us = t.elapsed().as_micros() as f64 / ITER as f64;

    println!("run_program bench (sanity assertions passed)");
    println!("  loop_sum(10_000)  {loop_us:9.2} µs/run  (interp loop: branchy scalar MIR)");
    println!("  fib(15)           {fib_us:9.2} µs/run  (cross-frame call stack, 1 973 calls)");
    println!(
        "  200-body table    {many_us:9.2} µs/run  (registration + entry pick, 0-iteration main)"
    );
}
