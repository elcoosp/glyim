use std::fs;
use std::path::PathBuf;
use glyim_codegen_llvm::compile_to_ir;

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

#[test] fn ir_return_42()   { run_snapshot("return_42"); }
#[test] fn ir_let_and_add() { run_snapshot("let_and_add"); }
#[test] fn ir_if_else()     { run_snapshot("if_else"); }
