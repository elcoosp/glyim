use glyim_cli::pipeline;

#[test]
fn generic_method_len() {
    let src = r#"
struct Vec<T> { data: *mut T, len: i64, cap: i64 }
impl<T> Vec<T> {
    fn len(self: Vec<T>) -> i64 { self.len }
}
fn main() -> i64 {
    let v: Vec<i64> = Vec { data: 0 as *mut i64, len: 42, cap: 0 };
    v.len()
}
"#;
    let result = pipeline::run_jit(src).unwrap();
    assert_eq!(result, 42);
}
