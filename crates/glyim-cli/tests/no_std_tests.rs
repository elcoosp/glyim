use glyim_cli::pipeline;

#[test]
fn no_std_simple_main() {
    let src = "no_std\nmain = () => 42";
    let result = pipeline::run_jit(src).unwrap();
    assert_eq!(result, 42);
}

#[test]
fn no_std_with_manual_alloc() {
    let src = r#"
no_std
main = () => {
    let ptr = __glyim_alloc(8) as *mut i64;
    *ptr = 99;
    let val = *ptr;
    __glyim_free(ptr as *mut u8);
    val
}
"#;
    let result = pipeline::run_jit(src).unwrap();
    assert_eq!(result, 99);
}

#[test]
fn no_std_does_not_include_prelude() {
    let src = "no_std\nmain = () => 1";
    let ir = glyim_codegen_llvm::compile_to_ir(src).unwrap();
    assert!(!ir.contains("glyim_println_int"), "no_std should not include println shim. Got:\n{ir}");
}
