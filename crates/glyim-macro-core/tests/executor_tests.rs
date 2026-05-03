use glyim_macro_core::executor::MacroExecutor;
use glyim_macro_vfs::LocalContentStore;
use std::sync::Arc;

#[test]
fn return_constant_works() {
    let executor = MacroExecutor::new();
    let wat = r#"
(module
  (func (export "expand") (param i32 i32 i32) (result i32)
    i32.const 42)
)
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let out = executor.execute(&wasm, b"hello").expect("constant");
    assert!(out.is_empty(), "no memory => no output bytes");
}

#[test]
fn identity_loop_copy() {
    let executor = MacroExecutor::new();
    let wat = r#"
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
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let input = b"hello";
    let out = executor.execute(&wasm, input).expect("loop copy");
    assert_eq!(out, input, "must copy input byte-for-byte");
}

#[test]
fn empty_input_loop() {
    let executor = MacroExecutor::new();
    let wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "expand") (param i32 i32 i32) (result i32)
    local.get 1)
)
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let out = executor.execute(&wasm, b"").expect("empty input");
    assert!(out.is_empty());
}

#[test]
fn cache_hit_with_local_store() {
    // Use a real LocalContentStore (on-disk CAS)
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().to_path_buf();
    let local = LocalContentStore::new(&store_path).expect("create local store");
    let executor = MacroExecutor::new_with_cache(Arc::new(local));

    // Identity loop that copies input to output
    let wat = r#"
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
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let input = b"cached_data";

    // First execution (cache miss — must run Wasm)
    let out1 = executor.execute(&wasm, input).expect("first execution");
    assert_eq!(out1, input, "first execution must copy input");

    // Second execution with same inputs (should hit cache)
    let out2 = executor.execute(&wasm, input).expect("second execution");
    assert_eq!(out2, out1, "cached result must match original");
    assert_eq!(out2, input, "cached result must copy input");
}

#[test]
fn cache_miss_with_local_store() {
    let dir = tempfile::tempdir().unwrap();
    let local = LocalContentStore::new(dir.path()).expect("create local store");
    let executor = MacroExecutor::new_with_cache(Arc::new(local));

    let wat = r#"
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
"#;
    let wasm = wat::parse_str(wat).unwrap();

    let in1 = b"aaa";
    let in2 = b"bbb";

    let out1 = executor.execute(&wasm, in1).expect("first run");
    let out2 = executor
        .execute(&wasm, in2)
        .expect("second run, different input");

    assert_eq!(out1, in1);
    assert_eq!(out2, in2);
    assert_ne!(
        out1, out2,
        "different inputs must produce different cached outputs"
    );
}
