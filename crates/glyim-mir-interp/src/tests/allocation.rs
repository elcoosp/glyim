use super::common::*;
use crate::*;
use glyim_mir::LocalIdx;
use glyim_test::test_ty_ctx;

#[test]
fn interpret_allocate_write_read() {
    let mut tcx_mut = test_ty_ctx();
    let val = 99;
    let body = build_allocation_body(&mut tcx_mut, val);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    let stored = interp.get_local_value(LocalIdx::from_usize(1)).unwrap();
    assert_eq!(stored, &InterpValue::Int(val));
}
