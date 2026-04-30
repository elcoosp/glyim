use glyim_cli::pipeline;
use std::path::PathBuf;

fn temp_g(content: &str) -> PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.g");
    std::fs::write(&path, content).unwrap();
    Box::leak(Box::new(dir));
    path
}

#[test]
fn e2e_main_42() {
    assert_eq!(pipeline::run(&temp_g("main = () => 42")).unwrap(), 42);
}
#[test]
fn e2e_add() {
    assert_eq!(pipeline::run(&temp_g("main = () => 1 + 2")).unwrap(), 3);
}
#[test]
fn e2e_block_last() {
    assert_eq!(pipeline::run(&temp_g("main = () => { 1 2 }")).unwrap(), 2);
}
#[test]
fn e2e_missing_main() {
    assert!(pipeline::run(&temp_g("fn other() { 1 }")).is_err());
}
#[test]
fn e2e_parse_error() {
    assert!(pipeline::run(&temp_g("main = +")).is_err());
}
#[test]
fn e2e_let_binding() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { let x = 42 }")).unwrap(),
        0
    );
}
#[test]
fn e2e_let_mut_assign() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { let mut x = 10\nx = x + 5\nx }")).unwrap(),
        15
    );
}
#[test]
fn e2e_if_true_branch() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { if true { 10 } else { 20 } }")).unwrap(),
        10
    );
}
#[test]
fn e2e_if_false_branch() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { if false { 10 } else { 20 } }")).unwrap(),
        20
    );
}
#[test]
fn e2e_if_without_else() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { if false { 42 } }")).unwrap(),
        0
    );
}
#[test]
fn e2e_else_if_chain() {
    assert_eq!(
        pipeline::run(&temp_g(
            "main = () => { if false { 1 } else if false { 2 } else { 3 } }"
        ))
        .unwrap(),
        3
    );
}
#[test]
fn e2e_println_int() {
    let _ = pipeline::run(&temp_g("main = () => { println(42) }")).unwrap();
}
#[test]
fn e2e_println_str() {
    let _ = pipeline::run(&temp_g(r#"main = () => { println("hello") }"#)).unwrap();
}
#[test]
fn e2e_assert_pass() {
    let _ = pipeline::run(&temp_g("main = () => { assert(1 == 1) }")).unwrap();
}
#[test]
fn e2e_assert_fail() {
    assert_ne!(
        pipeline::run(&temp_g("main = () => { assert(0) }")).unwrap(),
        0
    );
}
#[test]
fn e2e_assert_fail_msg() {
    assert_ne!(
        pipeline::run(&temp_g(r#"main = () => { assert(0, "oops") }"#)).unwrap(),
        0
    );
}
#[test]
fn e2e_bool() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { if true { 10 } else { 20 } }")).unwrap(),
        10
    );
}
#[test]
fn e2e_float() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { let x = 3.14; 1 }")).unwrap(),
        1
    );
}
#[test]
fn e2e_enum() {
    assert_eq!(
        pipeline::run(&temp_g(
            "enum Color { Red, Green, Blue }\nmain = () => { let c = Color::Green; 1 }"
        ))
        .unwrap(),
        1
    );
}
#[test]
fn e2e_struct() {
    assert_eq!(
        pipeline::run(&temp_g(
            "struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; 42 }"
        ))
        .unwrap(),
        42
    );
}
#[test]
fn e2e_match() {
    assert_eq!(pipeline::run(&temp_g("enum Color { Red, Green, Blue }\nmain = () => { let c = Color::Red; match c { Color::Red => 1, Color::Green => 2, Color::Blue => 3 } }")).unwrap(), 1);
}
#[test]
fn e2e_some_and_none() {
    assert_eq!(
        pipeline::run(&temp_g(
            "main = () => { let m = Some(42); match m { Some(v) => v, None => 0 } }"
        ))
        .unwrap(),
        42
    );
}
#[test]
fn e2e_ok_and_err() {
    assert_eq!(
        pipeline::run(&temp_g(
            "main = () => { let r = Ok(42); match r { Ok(v) => v, Err(_) => 0 } }"
        ))
        .unwrap(),
        42
    );
}
#[test]
fn e2e_macro_identity() {
    assert_eq!(
        pipeline::run(&temp_g(
            "@identity fn transform(expr: Expr) -> Expr { return expr } main = () => @identity(99)"
        ))
        .unwrap(),
        99
    );
}
#[test]
fn e2e_arrow() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => { let r = Ok(42)?; r }")).unwrap(),
        42
    );
}
// v0.4.0
#[test]
fn e2e_generic_identity() {
    let _ = pipeline::run(&temp_g("fn id<T>(x: T) -> T { x }\nmain = () => id(42)")).unwrap();
}
#[test]
fn e2e_generic_struct() {
    assert_eq!(pipeline::run(&temp_g("struct Container<T> { value: T }\nmain = () => { let c = Container { value: 42 }; c.value }")).unwrap(), 42);
}
#[test]
#[ignore]
fn e2e_tuple() {
    let src = "main = () => { let p = (1, 2); p._0 }";
    let _result = pipeline::run(&temp_g(src)).unwrap();
}
#[test]
fn e2e_impl_method() {
    let src = "struct Point { x, y }\nimpl Point {\n    fn zero() -> Point { Point { x: 0, y: 0 } }\n}\nmain = () => { let p = Point::zero(); p.x }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 0);
}
#[test]
fn e2e_cast_int_to_float() {
    let _ = pipeline::run(&temp_g("main = () => 42 as f64")).unwrap();
}
#[test]
fn e2e_prelude_some() {
    let _ = pipeline::run(&temp_g("main = () => Some(42)")).unwrap();
}
#[test]
fn e2e_prelude_result() {
    let _ = pipeline::run(&temp_g("main = () => Ok(100)")).unwrap();
}
#[test]
fn e2e_enum_match_prelude() {
    assert_eq!(
        pipeline::run(&temp_g(
            "main = () => { let m = Some(42); match m { Some(v) => v, None => 0 } }"
        ))
        .unwrap(),
        42
    );
}
#[test]
fn e2e_invalid_cast_fails() {
    assert!(pipeline::run(&temp_g("main = () => 42 as Str")).is_err());
}
#[test]
fn e2e_wrong_field_fails() {
    assert!(pipeline::run(&temp_g(
        "struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; p.z }"
    ))
    .is_err());
}
#[test]
#[ignore]
fn e2e_generic_edge() {
    let src = "struct Edge<T> { from: T, to: T }\nimpl<T> Edge<T> {\n    fn new(from: T, to: T) -> Edge<T> { Edge { from, to } }\n}\nfn main() -> i64 {\n    let e: Edge<i64> = Edge::new(0, 100)\n    let (from, to) = (e.from, e.to)\n    from - to\n}";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), -100);
}

#[test]
fn e2e_test_should_panic_passes() {
    let input = temp_g("#[test(should_panic)]\nfn panics() { 1 }");
    let summary = pipeline::run_tests(&input, None, false).unwrap();
    assert_eq!(summary.passed(), 1, "should_panic test should pass");
    assert_eq!(summary.exit_code(), 0);
}

#[test]
fn e2e_test_should_panic_fails_on_zero() {
    let input = temp_g("#[test(should_panic)]\nfn no_panic() { 0 }");
    let summary = pipeline::run_tests(&input, None, false).unwrap();
    assert_eq!(
        summary.failed(),
        1,
        "should_panic test that returns 0 should fail"
    );
    assert_eq!(summary.exit_code(), 1);
}

#[test]
fn e2e_test_filter() {
    let input = temp_g("#[test]\nfn a() { 0 }\n#[test]\nfn b() { 1 }");
    let summary = pipeline::run_tests(&input, Some("b"), false).unwrap();
    assert_eq!(summary.total(), 1);
    assert_eq!(summary.failed(), 1);
}

#[test]
fn e2e_test_filter_no_match() {
    let input = temp_g("#[test]\nfn a() { 0 }");
    let result = pipeline::run_tests(&input, Some("nonexistent"), false);
    assert!(result.is_err());
    let msg = format!("{:?}", result.unwrap_err());
    assert!(
        msg.contains("no #[test]"),
        "error should mention no test functions: {msg}"
    );
}

#[test]
fn e2e_type_error_unknown_field() {
    let input = temp_g("struct Point { x }\nmain = () => { let p = Point { x: 1 }; p.y }");
    let result = pipeline::run(&input);
    assert!(result.is_err());
}

#[test]
fn e2e_type_error_invalid_cast() {
    let input = temp_g("main = () => 42 as Str");
    let result = pipeline::run(&input);
    assert!(result.is_err());
}

#[test]
#[ignore]
fn e2e_type_error_int_plus_bool() {
    let input = temp_g("main = () => 1 + true");
    let result = pipeline::run(&input);
    assert!(result.is_err());
}

#[test]
fn e2e_type_error_missing_main() {
    let input = temp_g("fn other() { 1 }");
    let result = pipeline::run(&input);
    assert!(result.is_err());
}

#[test]
fn e2e_type_error_non_exhaustive_match() {
    let input = temp_g("enum Color { Red, Green, Blue }\nmain = () => match Color::Red { _ => 0 }");
    let result = pipeline::run(&input);
    assert!(result.is_ok());
}

#[test]
fn e2e_size_of_i64() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => __size_of::<i64>()")).unwrap(),
        8
    );
}

#[test]
fn e2e_size_of_unit() {
    assert_eq!(
        pipeline::run(&temp_g("main = () => __size_of::<()>()")).unwrap(),
        8
    );
}

#[test]
fn e2e_bool_if_rejects_int_condition() {
    let src = "fn main() -> i64 { let x = 5; if x { 1 } else { 0 } }";
    assert!(pipeline::run(&temp_g(src)).is_err());
}

#[test]
fn e2e_float_arithmetic_no_crash() {
    let src = "fn main() -> i64 { let x: f64 = 3.0; let y: f64 = x + 2.0; 1 }";
    assert!(pipeline::run(&temp_g(src)).is_ok());
}

#[test]
fn e2e_extern_block_with_ptr_param() {
    let src =
        "extern { fn write(fd: i64, buf: *const u8, len: i64) -> i64; }\nfn main() -> i64 { 0 }";
    assert!(pipeline::run(&temp_g(src)).is_ok());
}

#[test]
fn e2e_assign_to_immutable_is_error() {
    let src = "fn main() -> i64 { let x = 5; x = 10; x }";
    assert!(pipeline::run(&temp_g(src)).is_err());
}

#[test]
fn e2e_struct_with_ptr_parse_and_typecheck() {
    let src = "struct Ptr { data: *mut i64 }\nmain = () => { 42 }";
    assert!(pipeline::run(&temp_g(src)).is_ok());
}

#[test]
#[ignore = "requires impl blocks with self"]
fn e2e_veci64_push_get() {
    let vec_src = include_str!("../../../stdlib/src/vec_i64.g");
    let main_code = r#"
main = () => {
    let v = VecI64::new();
    v.push(10);
    v.push(20);
    v.push(30);
    let x = v.get(1);
    x
}
"#;
    let full_src = format!("{}\n{}", vec_src, main_code);
    let input = temp_g(&full_src);
    match pipeline::run(&input) {
        Ok(v) => assert_eq!(v, 20),
        Err(e) => {
            eprintln!("Vec test error: {:?}", e);
            panic!("Vec test failed");
        }
    }
}

#[test]
fn e2e_generic_identity_call() {
    let src = "fn id<T>(x: T) -> T { x }\nfn main() -> i64 { id(42) }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 42);
}

// ── Monomorphization verification tests ──────────────────────────

#[test]
fn e2e_mono_generic_fn_discovered_without_call_type_args() {
    let src = "fn id<T>(x: T) -> T { x }\nfn main() -> i64 { id(42) }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 42);
}

#[test]
fn e2e_mono_non_generic_param_before_generic() {
    let src = "fn wrap<T>(label: i64, value: T) -> T { value }\nfn main() -> i64 { wrap(0, 99) }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 99);
}

#[test]
fn e2e_mono_two_instantiations_same_fn() {
    let src = "fn id<T>(x: T) -> T { x }\nfn main() -> i64 { let a = id(42); let b = id(true); if b { a } else { 0 } }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 42);
}

#[test]
fn e2e_mono_generic_fn_with_two_type_params() {
    let src = "fn pair<A,B>(a: A, b: B) -> B { b }\nfn main() -> i64 { pair(1, 42) }";
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 42);
}

#[test]
fn e2e_while_loop() {
    let src = r#"
main = () => {
    let mut i = 0;
    let mut sum = 0;
    while i < 5 {
        sum = sum + i;
        i = i + 1;
    };
    sum
}
"#;
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 10);
}

#[test]
fn e2e_veci64_impl() {
    let src = r#"
struct VecI64 { data: *mut u8, len: i64, cap: i64 }

impl VecI64 {
    fn new() -> VecI64 { VecI64 { data: 0 as *mut u8, len: 0, cap: 0 } }
    fn len(&self) -> i64 { self.len }
}

main = () => {
    let v = VecI64::new();
    v.len()
}
"#;
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 0);
}
#[test]
fn e2e_generic_wrapper_i64() {
    let src = r#"
struct Wrapper<T> { value: T }
impl<T> Wrapper<T> { fn new(v: T) -> Wrapper<T> { Wrapper { value: v } } }
main = () => {
    let w = Wrapper::new(42);
    w.value
}
"#;
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 42);
}

#[test]
fn e2e_generic_wrapper_bool() {
    let src = r#"
struct Wrapper<T> { value: T }
impl<T> Wrapper<T> { fn new(v: T) -> Wrapper<T> { Wrapper { value: v } } }
main = () => {
    let w = Wrapper::new(true);
    if w.value { 1 } else { 0 }
}
"#;
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 1);
}

#[test]
fn e2e_vec_generic_push_get() {
    let src = r#"
struct Vec<T> { data: *mut u8, len: i64, cap: i64 }
impl<T> Vec<T> { fn new() -> Vec<T> { Vec { data: 0 as *mut u8, len: 0, cap: 0 } } }
main = () => { let v = Vec::new(); v.len }
"#;
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 0);
}


#[test]
fn e2e_string_generic_len() {
    let vec_src = include_str!("../../../stdlib/src/vec.g");
    let string_src = include_str!("../../../stdlib/src/string.g");
    let main_code = r#"
main = () => {
    let s = String::new();
    s.len()
}
"#;
    let full_src = format!("{}\n{}\n{}", vec_src, string_src, main_code);
    let input = temp_g(&full_src);
    assert_eq!(pipeline::run(&input).unwrap(), 0);

#[test]
fn e2e_vec_generic_len() {
    let src = r#"
struct Vec<T> { data: *mut u8, len: i64, cap: i64 }
impl<T> Vec<T> {
    fn new() -> Vec<T> { Vec { data: 0 as *mut u8, len: 0, cap: 0 } }
    fn len(&self) -> i64 { self.len }
}
main = () => { let v = Vec::new(); v.len() }
"#;
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 0);
}


#[test]
fn e2e_vec_generic_push() {
    let src = r#"
struct Vec<T> { data: *mut u8, len: i64, cap: i64 }
impl<T> Vec<T> {
    fn new() -> Vec<T> { Vec { data: 0 as *mut u8, len: 0, cap: 0 } }
    fn push(&mut self, value: T) {
        let elem_size = 8;
        if self.len == self.cap {
            let new_cap = if self.cap == 0 { 8 } else { self.cap * 2 };
            self.cap = new_cap;
        };
        let dst = __ptr_offset(self.data, self.len * elem_size) as *mut i64;
        *dst = value;
        self.len = self.len + 1;
    }
    fn get(&self, index: i64) -> i64 {
        if index >= self.len { 0 }
        else { *(__ptr_offset(self.data, index * 8) as *mut i64) }
    }
}
main = () => {
    let v = Vec::new();
    v.push(10);
    v.push(20);
    v.push(30);
    v.get(1)
}
"#;
    match pipeline::run(&temp_g(src)) {
        Ok(v) => assert_eq!(v, 20),
        Err(e) => {
            eprintln!("Push test error: {:?}", e);
            panic!("Push test failed");
        }
    }
}


#[test]
fn e2e_string_generic_len() {
    let src = r#"
struct Vec<T> { data: *mut u8, len: i64, cap: i64 }
impl<T> Vec<T> { fn new() -> Vec<T> { Vec { data: 0 as *mut u8, len: 0, cap: 0 } } }
struct String { vec: Vec<u8> }
impl String {
    fn new() -> String { String { vec: Vec::new() } }
    fn len(&self) -> i64 { self.vec.len() }
}
main = () => { let s = String::new(); s.len() }
"#;
    assert_eq!(pipeline::run(&temp_g(src)).unwrap(), 0);
}

}
