/// Generate source code for a single test binary that runs one test determined
/// by the GLYIM_TEST environment variable.
pub fn inject_single_test(source: &str, test_name: &str) -> String {
    let mut out = String::new();
    out.push_str(source);
    out.push_str("\n\n");
    out.push_str(&format!(
        "fn main() -> i64 {{\n    let result = {}();\n    result\n}}\n",
        test_name
    ));
    out
}
