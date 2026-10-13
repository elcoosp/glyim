//! Compiler-synthetic sentinel calls must fail loudly.
//!
//! `check_expr.rs` resolves a handful of methods (`into`, `to_string`,
//! `clone`, `drop`) to `FnDefId(u32::MAX - k)` when there is no real
//! `FnDef`. Lowering assumes those calls sit only on *dead* branches and
//! replaces them with an `Error` constant. When one lands on a live path,
//! evaluating it as a zero value silently produces wrong output — this is
//! exactly how `.to_string()` returned 0 for every String in the compiler.
//!
//! Invariant under test: reaching such a call panics, never returns.

use crate::*;
use glyim_core::{CrateId, DefId, FnDefId, IndexVec, LocalDefId, Mutability};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Substitution, Ty};

fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}

fn local_decl(ty: Ty) -> LocalDecl {
    LocalDecl {
        ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// A body whose only block calls `FnDefId(fn_id_raw)` and returns.
fn sentinel_call_body(fn_id_raw: u32) -> Body {
    let mut body = Body::dummy(dummy_def_id());
    let ret_local = LocalIdx::from_raw(0);
    let cont = BasicBlockIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT)]);
    body.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Call {
                    func: Operand::Constant(MirConst {
                        kind: MirConstKind::Fn(FnDefId::from_raw(fn_id_raw), Substitution::empty()),
                        ty: Ty::ERROR,
                        span: Span::DUMMY,
                    }),
                    args: vec![],
                    destination: Place::new(ret_local),
                    target: Some(cont),
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

/// Every sentinel slot (`u32::MAX - 1 ..= u32::MAX - 4`) must panic rather
/// than evaluate to a value. A future method resolved to a sentinel that
/// reaches a live path fails here instead of silently returning 0.
#[test]
fn sentinel_call_panics_instead_of_returning_zero() {
    let tcx = test_ty_ctx().freeze();
    for k in 1u32..=4 {
        let raw = u32::MAX - k;
        let body = sentinel_call_body(raw);
        let mut interp = Interpreter::new(&tcx);
        let result = interp.run_body(&body);
        assert!(
            matches!(result, Err(InterpError::Panic(_))),
            "sentinel FnDefId {raw} (u32::MAX - {k}) must panic, not return; \
             got {result:?}. A compiler-synthetic sentinel on a live path is \
             a bug — the value it produces is poison."
        );
    }
}

/// A sentinel call must not be *silently* absorbed by the builtin dispatcher
/// either. (The dispatcher's range check once let `>= 9_000` through, which
/// includes the sentinels.)
#[test]
fn sentinel_is_not_treated_as_a_builtin() {
    let tcx = test_ty_ctx().freeze();
    let raw = u32::MAX - 2;
    let body = sentinel_call_body(raw);
    let mut interp = Interpreter::new(&tcx);
    // Must not complete successfully with any value.
    assert!(
        interp.run_body(&body).is_err(),
        "sentinel FnDefId {raw} completed successfully — the builtin \
         dispatcher swallowed a sentinel"
    );
}
