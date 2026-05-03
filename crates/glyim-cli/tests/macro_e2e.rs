use glyim_macro_core::executor::MacroExecutor;
use glyim_macro_core::registry::MacroRegistry;
use glyim_macro_vfs::LocalContentStore;
use std::sync::Arc;

/// Simple identity macro in WAT: copies input bytes to output and returns length.
fn identity_wat() -> &'static str {
    r#"
(module
  (memory (export "memory") 1)
  (func (export "expand") (param i32 i32 i32) (result i32)
    (local i32)
    i32.const 0
    local.set 3
    loop (result i32)
      local.get 3
      local.get 1
      i32.lt_s
      if (result i32)
        local.get 2
        local.get 3
        i32.add
        local.get 0
        local.get 3
        i32.add
        i32.load8_u
        i32.store8
        local.get 3
        i32.const 1
        i32.add
        local.set 3
        br 1
      else
        local.get 1
      end
    end)
)
"#
}

#[test]
fn e2e_macro_registry_and_execution() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalContentStore::new(dir.path()).unwrap();
    let store = Arc::new(store);

    // Setup registry with identity macro
    let mut registry = MacroRegistry::new(store.clone());
    let wasm = wat::parse_str(identity_wat()).expect("parse identity wat");
    registry.register("identity", wasm);

    // Setup executor with caching
    let executor = MacroExecutor::new_with_cache(store);

    // Execute
    let macro_wasm = registry.get("identity").expect("macro registered");
    let input = b"hello glyim macros!";
    let result = executor.execute(macro_wasm, input).expect("execution");
    assert_eq!(result, input, "identity macro must return input unchanged");
}

#[test]
fn e2e_cache_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalContentStore::new(dir.path()).unwrap();
    let store = Arc::new(store);

    let mut registry = MacroRegistry::new(store.clone());
    let wasm = wat::parse_str(identity_wat()).expect("parse identity wat");
    registry.register("identity", wasm);

    let executor = MacroExecutor::new_with_cache(store);
    let input = b"cache test data";
    let macro_wasm = registry.get("identity").expect("macro registered");

    // First execution (cache miss)
    let out1 = executor.execute(macro_wasm, input).expect("first exec");
    assert_eq!(out1, input);

    // Second execution (cache hit)
    let out2 = executor.execute(macro_wasm, input).expect("second exec");
    assert_eq!(out2, out1);
}

#[test]
fn e2e_different_inputs_different_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalContentStore::new(dir.path()).unwrap();
    let store = Arc::new(store);

    let mut registry = MacroRegistry::new(store.clone());
    let wasm = wat::parse_str(identity_wat()).expect("parse identity wat");
    registry.register("identity", wasm);

    let executor = MacroExecutor::new_with_cache(store);
    let macro_wasm = registry.get("identity").expect("macro registered");

    let out_a = executor.execute(macro_wasm, b"aaa").expect("exec a");
    let out_b = executor.execute(macro_wasm, b"bbb").expect("exec b");

    assert_eq!(out_a, b"aaa");
    assert_eq!(out_b, b"bbb");
    assert_ne!(out_a, out_b);
}
