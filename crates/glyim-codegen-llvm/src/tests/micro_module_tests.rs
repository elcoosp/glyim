use crate::dispatch::DispatchTable;
use crate::micro_module::MicroModuleManager;
use inkwell::context::Context;
use std::sync::Arc;

#[test]
fn micro_module_manager_create() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);
    assert_eq!(manager.module_count(), 0);
}

#[test]
fn micro_module_manager_create_module() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let mut manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);
    let module = manager.create_module_for_item("add_fn");
    assert!(module.is_some());
    assert_eq!(manager.module_count(), 1);
}

#[test]
fn micro_module_manager_create_duplicate_module_replaces() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let mut manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);
    let _m1 = manager.create_module_for_item("fn_a");
    let _m2 = manager.create_module_for_item("fn_a");
    assert_eq!(manager.module_count(), 1);
}

#[test]
fn micro_module_manager_create_multiple_modules() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let mut manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);
    let _m1 = manager.create_module_for_item("fn_a");
    let _m2 = manager.create_module_for_item("fn_b");
    let _m3 = manager.create_module_for_item("fn_c");
    assert_eq!(manager.module_count(), 3);
}

#[test]
fn micro_module_manager_remove_module() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let mut manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);
    let _m = manager.create_module_for_item("temp");
    assert_eq!(manager.module_count(), 1);
    manager.remove_module("temp");
    assert_eq!(manager.module_count(), 0);
}

#[test]
fn micro_module_manager_contains_module() {
    let context = Context::create();
    let dispatch = Arc::new(DispatchTable::new());
    let mut manager = MicroModuleManager::new(&context, "glyim_micro", dispatch);
    assert!(!manager.contains("fn_a"));
    let _m = manager.create_module_for_item("fn_a");
    assert!(manager.contains("fn_a"));
}
