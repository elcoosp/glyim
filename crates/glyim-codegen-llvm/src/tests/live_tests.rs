use crate::live::DoubleBufferedJIT;
use crate::dispatch::DispatchTable;
use glyim_interner::Interner;
use std::sync::Arc;

#[test]
fn double_buffered_jit_create() {
    let dispatch = Arc::new(DispatchTable::new());
    let jit = DoubleBufferedJIT::new(dispatch);
    assert_eq!(jit.staged_count(), 0);
}

#[test]
fn double_buffered_jit_stage_item() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut jit = DoubleBufferedJIT::new(dispatch);
    let mut interner = Interner::new();
    let sym = interner.intern("add");
    jit.stage_item(sym);
    assert_eq!(jit.staged_count(), 1);
}

#[test]
fn double_buffered_jit_stage_multiple_items() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut jit = DoubleBufferedJIT::new(dispatch);
    let mut interner = Interner::new();
    jit.stage_item(interner.intern("add"));
    jit.stage_item(interner.intern("sub"));
    jit.stage_item(interner.intern("mul"));
    assert_eq!(jit.staged_count(), 3);
}

#[test]
fn double_buffered_jit_commit_clears_staging() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut jit = DoubleBufferedJIT::new(dispatch);
    let mut interner = Interner::new();
    jit.stage_item(interner.intern("add"));
    assert_eq!(jit.staged_count(), 1);
    jit.commit();
    assert_eq!(jit.staged_count(), 0);
}

#[test]
fn double_buffered_jit_commit_updates_dispatch() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut jit = DoubleBufferedJIT::new(dispatch.clone());
    let mut interner = Interner::new();
    let sym = interner.intern("add");
    dispatch.update(sym, 0xDEAD);
    jit.stage_item(sym);
    jit.commit();
    assert_eq!(dispatch.get_address(sym), 0xDEAD);
}

#[test]
fn double_buffered_jit_is_send() {
    fn assert_bounds<T: Send>() {}
    assert_bounds::<DoubleBufferedJIT>();
}
