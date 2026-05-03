use glyim_codegen_llvm::compile_to_wasm;

#[test]
fn wasm_module_has_correct_magic() {
    let source = "fn main() -> i64 { 42 }";
    let wasm = compile_to_wasm(source, "wasm32-wasi").expect("compile");
    assert_eq!(&wasm[0..4], b"\x00asm", "Missing Wasm magic");
    assert_eq!(&wasm[4..8], &[0x01, 0x00, 0x00, 0x00], "Expected version 1");
}
