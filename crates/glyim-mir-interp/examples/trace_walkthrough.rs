//! M1 `InterpObserver` consumer example.
//!
//! Demonstrates the observability seam on [`Interpreter`]: a caller body
//! calls a returning callee, and a [`TraceCollector`] records every dispatch
//! event — basic-block entries bracketing a symmetric `CallEnter` /
//! `CallReturn` pair — exactly as the dispatch loop fires them. The
//! interpreter had no such hook before M1; consumers reached into its
//! internals to trace a run. This example is the supported path.
//!
//! # Run
//!
//! ```sh
//! cargo run -p glyim-mir-interp --example trace_walkthrough
//! ```
//!
//! Expected output: one line per `TraceEvent` — `bb_enter owner=… bb=…`,
//! `call_enter callee=…`, `call_return callee=…` — in dispatch order, then
//! the callee's result (`42`).

use glyim_core::{CrateId, DefId, FnDefId, IndexVec, IntTy, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_mir_interp::{InterpValue, Interpreter, TraceEvent};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Substitution, Ty, TyCtxMut, TyKind};

/// `DefId(0, raw)`.
fn id(raw: u32) -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(raw))
}

/// `fn() -> i32 { _0 = val; return }` — a single-block returning callee.
fn returning_body(tcx: &mut TyCtxMut, owner: DefId, val: i128) -> Body {
    let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(owner);
    body.locals = IndexVec::from_raw(vec![LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    }]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(val),
                    ty: i32_ty,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    body
}

/// A caller owned by `caller` whose `bb0` calls the callee owned by
/// `callee_raw`, landing the result in local `_1`, then `bb1` returns.
fn caller_body(tcx: &mut TyCtxMut, caller: DefId, callee_raw: u32) -> Body {
    let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(caller);
    body.locals = IndexVec::from_raw(vec![
        LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ]);
    body.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Call {
                    func: Operand::Constant(MirConst {
                        kind: MirConstKind::Fn(
                            FnDefId::from_raw(callee_raw),
                            Substitution::empty(),
                        ),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    }),
                    args: vec![],
                    destination: Place::new(LocalIdx::from_raw(1)),
                    target: Some(BasicBlockIdx::from_raw(1)),
                    cleanup: None,
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        },
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        },
    ]);
    body
}

fn main() {
    let mut tcx_mut = test_ty_ctx();
    let callee_id = id(1);
    let callee = returning_body(&mut tcx_mut, callee_id, 42);
    let caller_id = id(0);
    let caller = caller_body(&mut tcx_mut, caller_id, 1);
    let tcx = tcx_mut.freeze();

    let mut interp = Interpreter::new(&tcx).with_trace_collector();
    interp.add_function(callee_id, callee);
    interp.run_body(&caller).unwrap();

    let result = interp
        .get_local_value(LocalIdx::from_raw(1))
        .map(|v| match v {
            InterpValue::Int(n) => *n,
            other => panic!("expected Int result, got {other:?}"),
        })
        .expect("call result landed in local 1");

    let events = interp
        .take_trace()
        .expect("with_trace_collector installed a TraceCollector");

    for ev in events {
        match ev {
            TraceEvent::BbEnter { owner, bb } => {
                println!("bb_enter  owner={owner:?}  bb={bb:?}");
            }
            TraceEvent::CallEnter { callee } => {
                println!("call_enter  callee={callee:?}");
            }
            TraceEvent::CallReturn { callee } => {
                println!("call_return callee={callee:?}");
            }
        }
    }
    println!("result = {result}");
}
