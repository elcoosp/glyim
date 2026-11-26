use super::common::*;
use crate::*;
use glyim_core::{CrateId, DefId, LocalDefId};

#[test]
fn step_limit_default() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let interp = Interpreter::new(&tcx);
    assert_eq!(interp.step_limit(), 1_000_000);
}

#[test]
fn recursion_limit_default() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let interp = Interpreter::new(&tcx);
    assert_eq!(interp.recursion_limit(), 256);
}

#[test]
fn interpret_infinite_loop_times_out() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let body = build_infinite_loop_body();
    let mut interp = Interpreter::new(&tcx).with_step_limit(10);
    let res = interp.run_body(&body);
    assert_eq!(res, Err(InterpError::TimedOut));
}

#[test]
fn interpret_deep_recursion_overflows() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let def_id = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0));
    let body = build_recursive_body(def_id);
    let mut interp = Interpreter::new(&tcx).with_recursion_limit(2);
    interp.add_function(def_id, body.clone());
    let res = interp.run_body(&body);
    assert_eq!(res, Err(InterpError::StackOverflow));
}
