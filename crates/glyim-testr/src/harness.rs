/// Generate source code for a single test binary that runs one test determined
/// by the GLYIM_TEST environment variable.
pub fn inject_single_test(source: &str, test_name: &str) -> String {
    let mut out = String::new();
    out.push_str(source);
    out.push_str("\n\n");
    let main_body = format!(
        "fn main() -> i64 {{\n    let result = {}();\n    result\n}}\n",
        test_name
    );
    eprintln!(
        "[test harness] generated source for test '{}':\n{}",
        test_name, main_body
    );
    out.push_str(&main_body);
    out
}
