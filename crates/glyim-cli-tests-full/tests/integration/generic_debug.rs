use glyim_compiler::pipeline;

#[test]
fn generic_method_return_i64() {
    let src = r#"
struct Vec<T> { data: *mut T, len: i64, cap: i64 }
impl<T> Vec<T> {
    fn get_or_default(self: Vec<T>, index: i64) -> i64 {
        if index >= self.len { -1 } else { self.len }
    }
}
fn main() -> i64 {
    let v: Vec<i64> = Vec { data: 0 as *mut i64, len: 0, cap: 0 };
    v.get_or_default(0)
}
"#;
    let result = pipeline::run_jit(src).unwrap();
    assert_eq!(result, -1);
}

#[test]
fn generic_method_return_none() {
    let src = r#"
struct Vec<T> { data: *mut T, len: i64, cap: i64 }
impl<T> Vec<T> {
    fn maybe_get(self: Vec<T>, index: i64) -> Option<T> {
        None
    }
}
fn main() -> i64 {
    let v: Vec<i64> = Vec { data: 0 as *mut i64, len: 0, cap: 0 };
    match v.maybe_get(0) {
        Some(x) => x,
        None => -1,
    }
}
"#;
    let result = pipeline::run_jit(src).unwrap();
    assert_eq!(result, -1);
}
