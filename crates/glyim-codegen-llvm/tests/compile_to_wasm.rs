use glyim_codegen_llvm::compile_to_wasm;

#[test]
fn compile_trivial_to_wasm() {
    let source = "fn main() -> i64 { 42 }";
    let wasm = compile_to_wasm(source, "wasm32-wasi").expect("compile");
    assert!(!wasm.is_empty(), "Wasm binary must not be empty");
    // Check magic bytes
    assert_eq!(&wasm[0..4], b"\x00asm", "Must start with Wasm magic");
}
