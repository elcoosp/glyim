//! TDD witness for the `InterpObserver` observability seam (M1).
//!
//! The MIR interpreter — the differential reference backend — had no
//! per-step observation hook: consumers (`glyip test`, `glyim-test`'s
//! `InterpRunner`, the `--emit=run` CLI) received only the final
//! `RunOutcome`/`InterpError`. The bytecode VM exposes execution through
//! its `*Sink` traits (see `glyim-bytecode-vm/src/lib.rs` §"Observability
//! seams"); this module pins the equivalent seam for the interpreter.
//!
//! The witness builds a caller body that calls a returning callee, runs it
//! with [`Interpreter::with_trace_collector`], and asserts the recorded
//! [`TraceEvent`] sequence — basic-block entries bracketing a symmetric
//! `CallEnter`/`CallReturn` pair — matches the dispatch loop exactly.

use crate::*;
use glyim_core::{CrateId, DefId, FnDefId, IndexVec, IntTy, LocalDefId, Mutability};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Substitution, Ty, TyCtxMut, TyKind};

/// `DefId(0, raw)` — the owner id convention used by the interpreter's
/// `resolve_callee` for `MirConstKind::Fn(FnDefId::from_raw(raw), _)`.
fn id(raw: u32) -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(raw))
}

/// `fn(ret_ty) -> i32 { _0 = val; return }` — a single-block callee that
/// materialises `val` in its return place and returns.
fn build_returning_body(tcx: &mut TyCtxMut, owner: DefId, val: i128) -> Body {
    let mut body = Body::dummy(owner);
    let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
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

/// A caller body owned by `caller` whose `bb0` calls the callee owned by
/// `callee_raw` (resolved via `FnDefId::from_raw(callee_raw)`), landing the
/// result in local `_1`, then `bb1` returns. Mirrors the shape of
/// `calls::interpret_function_call` but with distinct owner ids so the
/// trace can distinguish caller from callee.
fn build_caller_body(tcx: &mut TyCtxMut, caller: DefId, callee_raw: u32) -> Body {
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

#[test]
fn observer_records_call_sequence() {
    let mut tcx_mut = test_ty_ctx();
    let callee_id = id(1);
    let callee_body = build_returning_body(&mut tcx_mut, callee_id, 42);
    let caller_id = id(0);
    let caller_body = build_caller_body(&mut tcx_mut, caller_id, 1);

    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx).with_trace_collector();
    interp.add_function(callee_id, callee_body);
    interp.run_body(&caller_body).unwrap();

    let val = interp
        .get_local_value(LocalIdx::from_raw(1))
        .expect("call result landed in local 1");
    assert_eq!(val, &InterpValue::Int(42), "callee result must still flow");

    let events = interp
        .take_trace()
        .expect("with_trace_collector installed a TraceCollector");

    let bb0 = BasicBlockIdx::from_raw(0);
    let bb1 = BasicBlockIdx::from_raw(1);
    let expected = vec![
        TraceEvent::BbEnter {
            owner: caller_id,
            bb: bb0,
        },
        TraceEvent::CallEnter { callee: callee_id },
        TraceEvent::BbEnter {
            owner: callee_id,
            bb: bb0,
        },
        TraceEvent::CallReturn { callee: callee_id },
        TraceEvent::BbEnter {
            owner: caller_id,
            bb: bb1,
        },
        TraceEvent::CallReturn { callee: caller_id },
    ];
    assert_eq!(
        events, expected,
        "observer must record the full call sequence: caller bb0 → call → \
         callee bb0 → callee return → caller bb1 → caller return"
    );
}

#[test]
fn noop_observer_is_default_and_silent() {
    // With no observer installed, interpretation is unchanged and
    // `take_trace` yields `None` (no TraceCollector to downcast).
    let mut tcx_mut = test_ty_ctx();
    let callee_id = id(1);
    let callee_body = build_returning_body(&mut tcx_mut, callee_id, 7);
    let caller_id = id(0);
    let caller_body = build_caller_body(&mut tcx_mut, caller_id, 1);

    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(callee_id, callee_body);
    interp.run_body(&caller_body).unwrap();

    assert!(
        interp.take_trace().is_none(),
        "no TraceCollector installed → take_trace is None"
    );
    let val = interp
        .get_local_value(LocalIdx::from_raw(1))
        .expect("call result landed in local 1");
    assert_eq!(val, &InterpValue::Int(7));
}

#[test]
fn custom_observer_impl_is_dispatched() {
    /// A hand-rolled observer that counts `CallEnter` events only — proves
    /// the trait is an open extension point, not a closed enum.
    struct CallCounter {
        enters: u32,
        returns: u32,
    }
    impl crate::InterpObserver for CallCounter {
        fn on_call_enter(&mut self, _callee: DefId) {
            self.enters += 1;
        }
        fn on_call_return(&mut self, _callee: DefId) {
            self.returns += 1;
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    let mut tcx_mut = test_ty_ctx();
    let callee_id = id(1);
    let callee_body = build_returning_body(&mut tcx_mut, callee_id, 42);
    let caller_id = id(0);
    let caller_body = build_caller_body(&mut tcx_mut, caller_id, 1);

    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx).with_observer(Box::new(CallCounter {
        enters: 0,
        returns: 0,
    }));
    interp.add_function(callee_id, callee_body);
    interp.run_body(&caller_body).unwrap();

    let obs = interp
        .take_observer()
        .expect("custom observer was installed");
    let counter = obs
        .as_any()
        .downcast_ref::<CallCounter>()
        .expect("downcast to the concrete CallCounter");
    // One callee call + the entry body's own return path: the callee call
    // is the only CallEnter (the entry body is invoked via `run_body`, not
    // a `Call` terminator); the callee and the entry body each Return once.
    assert_eq!(counter.enters, 1, "one Call terminator fired");
    assert_eq!(counter.returns, 2, "callee + entry body each return once");
}
