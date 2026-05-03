use glyim_macro_core::executor::MacroExecutor;
use glyim_macro_core::wasm_interface::{serialize_expr, deserialize_expr};
use glyim_hir::HirExpr;
use glyim_diag::Span;
use glyim_hir::types::ExprId;
use glyim_interner::Symbol;

#[test]
fn typed_executor_identity_roundtrip() {
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
    let wasm = wat::parse_str(wat).expect("parse wat");

    let expr = HirExpr::IntLit {
        id: ExprId::new(0),
        value: 42,
        span: Span::new(0, 2),
    };
    let bytes = serialize_expr(&expr);
    let result_bytes = executor.execute(&wasm, &bytes).expect("execute typed macro");
    let result_expr = deserialize_expr(&result_bytes).expect("deserialize result");
    match result_expr {
        HirExpr::IntLit { value, .. } => assert_eq!(value, 42),
        _ => panic!("expected IntLit"),
    }
}
