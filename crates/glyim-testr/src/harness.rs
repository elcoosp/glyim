pub fn inject_harness(original: &str, tests: &[String]) -> String {
    let mut out = String::new();

    out.push_str(original);
    out.push_str("\n\n");

    out.push_str(r#"
extern {
    fn __glyim_getenv(name: *const u8) -> *const u8;
    fn __glyim_str_eq(a: *const u8, b: *const u8) -> i64;
    fn write(fd: i32, buf: *const u8, len: i64) -> i64;
}

fn main() -> i64 {
    let name_ptr = __glyim_getenv("GLYIM_TEST\0" as *const u8);
    if name_ptr == (0 as *const u8) {
        write(2, "error: GLYIM_TEST not set\n\0" as *const u8, 26);
        return 1;
    }
"#);

    for test in tests {
        let test_null = format!("{}\0", test);
        let pass_len = 5 + test.len() + 1; // "PASS <name>\n"
        let fail_len = 5 + test.len() + 1;

        out.push_str(&format!(
            "    if __glyim_str_eq(name_ptr, \"{}\" as *const u8) != 0 {{\n",
            test_null
        ));
        out.push_str(&format!("        let result = {}();\n", test));
        out.push_str("        if result == 0 {\n");
        out.push_str(&format!(
            "            write(1, \"PASS {}\\n\0\" as *const u8, {});\n",
            test, pass_len
        ));
        out.push_str("            return 0;\n");
        out.push_str("        } else {\n");
        out.push_str(&format!(
            "            write(1, \"FAIL {}\\n\0\" as *const u8, {});\n",
            test, fail_len
        ));
        out.push_str("            return result;\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
    }

    out.push_str(r#"
    write(2, "error: unknown test\n\0" as *const u8, 20);
    return 1;
}
"#);

    out
}
