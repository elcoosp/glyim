use crate::tiered::{TieredCompiler, ExecutionTier};
use crate::dispatch::DispatchTable;
use glyim_interner::Interner;
use std::sync::Arc;

#[test]
fn tiered_compiler_new_function_starts_at_tier0() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let tiered = TieredCompiler::new(dispatch, 100);
    let name = interner.intern("add");
    assert_eq!(tiered.execution_tier(name), ExecutionTier::Tier0);
}

#[test]
fn tiered_compiler_execute_increments_count() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let tiered = TieredCompiler::new(dispatch, 100);
    let name = interner.intern("add");
    assert_eq!(tiered.execution_count(name), 0);
    tiered.record_execution(name);
    assert_eq!(tiered.execution_count(name), 1);
    tiered.record_execution(name);
    assert_eq!(tiered.execution_count(name), 2);
}

#[test]
fn tiered_compiler_promote_after_threshold() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let tiered = TieredCompiler::new(dispatch, 5);
    let name = interner.intern("hot_fn");
    for _ in 0..4 {
        tiered.record_execution(name);
    }
    assert_eq!(tiered.execution_tier(name), ExecutionTier::Tier0);
    tiered.record_execution(name);
    assert_eq!(tiered.execution_tier(name), ExecutionTier::Tier1);
}

#[test]
fn tiered_compiler_promote_idle() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let tiered = TieredCompiler::new(dispatch, 1000);
    let name = interner.intern("lazy_fn");
    tiered.record_execution(name);
    assert_eq!(tiered.execution_tier(name), ExecutionTier::Tier0);
    let promoted = tiered.promote_all();
    assert_eq!(promoted.len(), 1);
    assert_eq!(tiered.execution_tier(name), ExecutionTier::Tier1);
}

#[test]
fn tiered_compiler_execution_tier_unknown() {
    let dispatch = Arc::new(DispatchTable::new());
    let mut interner = Interner::new();
    let tiered = TieredCompiler::new(dispatch, 100);
    let unknown = interner.intern("unknown");
    assert_eq!(tiered.execution_tier(unknown), ExecutionTier::Tier0);
    assert_eq!(tiered.execution_count(unknown), 0);
}

#[test]
fn execution_tier_ordering() {
    assert!(ExecutionTier::Tier0 < ExecutionTier::Tier1);
}

#[test]
fn tiered_compiler_is_send() {
    fn assert_bounds<T: Send>() {}
    assert_bounds::<TieredCompiler>();
}
