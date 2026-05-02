use glyim_codegen_llvm::compile_to_ir;
use std::fs;
use std::path::PathBuf;

fn snap_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("ir_snapshots")
}

fn run_snapshot(name: &str) {
    let source_path = snap_dir().join(format!("{name}.g"));
    let source = fs::read_to_string(&source_path).unwrap();
    let ir = compile_to_ir(&source).unwrap();
    insta::assert_snapshot!(name, ir);
}

#[test]
fn ir_return_42() {
    run_snapshot("return_42");
}
#[test]
fn ir_let_and_add() {
    run_snapshot("let_and_add");
}
#[test]
fn ir_if_else() {
    run_snapshot("if_else");
}
#[test]
fn ir_while_loop() { run_snapshot("while_loop"); }
#[test]
fn ir_struct_lit() { run_snapshot("struct_lit"); }
#[test]
fn ir_enum_variant() { run_snapshot("enum_variant"); }
#[test]
fn ir_match_expr() { run_snapshot("match_expr"); }
#[test]
fn ir_field_access() { run_snapshot("field_access"); }
#[test]
fn ir_deref_expr() { run_snapshot("deref_expr"); }
#[test]
fn ir_assert_pass() { run_snapshot("assert_pass"); }
#[test]
fn ir_call_fn() { run_snapshot("call_fn"); }
#[test]
fn ir_float_ops() { run_snapshot("float_ops"); }
#[test]
fn ir_generic_fn() { run_snapshot("generic_fn"); }
#[test]
fn ir_tuple_lit() { run_snapshot("tuple_lit"); }
