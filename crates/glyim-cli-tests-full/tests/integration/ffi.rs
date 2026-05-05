#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_extern_block_with_ptr_param() {
    let src =
        "extern { fn write(fd: i64, buf: *const u8, len: i64) -> i64; }\nfn main() -> i64 { 0 }";
    assert!(pipeline::run(&temp_g(src), None).is_ok());
}

#[test]
fn e2e_extern_write_i32_fd() {
    let src = "extern { fn write(fd: i32, buf: *const u8, len: i64) -> i64; } main = () => { write(1, 0 as *const u8, 0) }";
    assert!(pipeline::run(&temp_g(src), None).is_ok());
}

#[test]
fn e2e_extern_method_write() {
    let io_src = include_str!("../../../../stdlib/src/io.g");
    let main_code = r#"
main = () => {
    let out = stdout();
    out.write(0 as *const u8, 0);
    42
}
"#;
    let full_src = format!(
        "{}
{}",
        io_src, main_code
    );
    let result = pipeline::run_jit(&full_src);
    assert!(result.is_ok(), "extern method write: {:?}", result.err());
    assert_eq!(result.unwrap(), 42);
}

