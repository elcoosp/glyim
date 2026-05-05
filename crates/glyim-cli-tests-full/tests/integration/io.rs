#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_println_int() {
    let _ = pipeline::run(&temp_g("main = () => { println(42) }"), None).unwrap();
}

#[test]
fn e2e_println_str() {
    let _ = pipeline::run(&temp_g(r#"main = () => { println("hello") }"#), None).unwrap();
}

#[test]
fn e2e_write_string_literal() {
    let src = r#"extern { fn write(fd: i32, buf: *const u8, len: i64) -> i64; }
main = () => {
    write(1, "hello\n", 6)
}"#;
    assert!(pipeline::run(&temp_g(src), None).is_ok());
}

#[test]
fn e2e_io_write_stdout_compile() {
    let io_src = include_str!("../../../../stdlib/src/io.g");
    let main_code = r#"
main = () => {
    let out = stdout();
    write(out.fd as i32, 0 as *const u8, 0)
}
"#;
    let full_src = format!("{}\n{}", io_src, main_code);
    let input = temp_g(&full_src);
    assert!(pipeline::run(&input, None).is_ok());
}

#[test]
fn e2e_io_write_stderr_compile() {
    let io_src = include_str!("../../../../stdlib/src/io.g");
    let main_code = r#"
main = () => {
    let err = stderr();
    write(err.fd as i32, 0 as *const u8, 0)
}
"#;
    let full_src = format!("{}\n{}", io_src, main_code);
    let input = temp_g(&full_src);
    assert!(pipeline::run(&input, None).is_ok());
}

#[test]
fn e2e_io_method_stdout_compile() {
    let io_src = include_str!("../../../../stdlib/src/io.g");
    let main_code = r#"
main = () => {
    let out = stdout();
    out.write(0 as *const u8, 0)
}
"#;
    let full_src = format!("{}\n{}", io_src, main_code);
    let input = temp_g(&full_src);
    assert!(pipeline::run(&input, None).is_ok());
}

#[test]
fn e2e_io_method_stderr_compile() {
    let io_src = include_str!("../../../../stdlib/src/io.g");
    let main_code = r#"
main = () => {
    let err = stderr();
    err.write(0 as *const u8, 0)
}
"#;
    let full_src = format!("{}\n{}", io_src, main_code);
    let input = temp_g(&full_src);
    assert!(pipeline::run(&input, None).is_ok());
}

#[test]
fn e2e_io_stdin_read_compile() {
    let io_src = include_str!("../../../../stdlib/src/io.g");
    let main_code = r#"
main = () => {
    let input = stdin();
    let buf = __glyim_alloc(16) as *mut u8;
    read(input.fd as i32, buf, 16)
}
"#;
    let full_src = format!("{}\n{}", io_src, main_code);
    let input = temp_g(&full_src);
    assert!(pipeline::run(&input, None).is_ok());
}

#[test]
fn e2e_io_stdout_compile() {
    let io_src = include_str!("../../../../stdlib/src/io.g");
    let main_code = r#"
main = () => {
    let out = stdout();
    write(out.fd, 0 as *const u8, 0)
}
"#;
    let full_src = format!("{}\n{}", io_src, main_code);
    let input = temp_g(&full_src);
    assert!(pipeline::run(&input, None).is_ok());
}

#[test]
fn e2e_io_stderr_compile() {
    let io_src = include_str!("../../../../stdlib/src/io.g");
    let main_code = r#"
main = () => {
    let err = stderr();
    write(err.fd, 0 as *const u8, 0)
}
"#;
    let full_src = format!("{}\n{}", io_src, main_code);
    let input = temp_g(&full_src);
    assert!(pipeline::run(&input, None).is_ok());
}

#[test]
fn e2e_io_write_compile() {
    let io_src = include_str!("../../../../stdlib/src/io.g");
    let main_code = r#"
main = () => {
    let out = stdout();
    out.write(0 as *const u8, 0);
    42
}
"#;
    let full_src = format!("{}\n{}", io_src, main_code);
    let input = temp_g(&full_src);
    assert!(pipeline::run(&input, None).is_ok());
}

#[test]
fn e2e_io_extern_write_compile() {
    let main_code = r#"
extern {
    fn write(fd: i64, buf: *const u8, count: i64) -> i64;
}
main = () => {
    let ptr = "hello" as *const u8;
    write(1, ptr, 0);
    42
}
"#;
    let input = temp_g(main_code);
    assert!(pipeline::run(&input, None).is_ok());
}

#[test]
fn e2e_println_stdout_captures_output() {
    let dir = tempfile::tempdir().unwrap();
    let main_g = dir.path().join("main.g");
    std::fs::write(&main_g, r#"main = () => { println("hello from test") }"#).unwrap();
    let result = pipeline::run(&main_g, None);
    // println returns 0 on success
    assert!(
        result.is_ok(),
        "println should compile and run: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), 0);
}

#[test]
fn e2e_io_write_method_compiles() {
    let io_src = include_str!("../../../../stdlib/src/io.g");
    let main_code = r#"
main = () => {
    let out = stdout();
    out.write("test\n" as *const u8, 5);
    0
}
"#;
    let full_src = format!("{}\n{}", io_src, main_code);
    let src_path = temp_g(&full_src);
    assert!(pipeline::run(&src_path, None).is_ok());
}

#[test]
fn e2e_println_int_var() {
    let src = "main = () => { let x = 123; println(x) }";
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "println int var: {:?}", result.err());
}

#[test]
fn e2e_println_str_var() {
    let src = r#"main = () => { let s = "hello"; println(s) }"#;
    let result = pipeline::run_jit(src);
    assert!(result.is_ok(), "println str var: {:?}", result.err());
}

