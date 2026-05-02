use glyim_macro_core::executor::MacroExecutor;

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
    local.set 3                       ;; i = 0
    loop (result i32)
      local.get 3
      local.get 1                     ;; compare i < len
      i32.lt_s
      if (result i32)
        local.get 2                    ;; dst base
        local.get 3
        i32.add
        local.get 0                    ;; src base
        local.get 3
        i32.add
        i32.load8_u
        i32.store8
        local.get 3
        i32.const 1
        i32.add
        local.set 3                    ;; i++
        br 1                           ;; continue loop
      else
        local.get 1                    ;; return len
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
    local.get 1)    ;; just return len (0)
)
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let out = executor.execute(&wasm, b"").expect("empty input");
    assert!(out.is_empty());
}
