#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_generic_identity() {
    let _ = pipeline::run(
        &temp_g("fn id<T>(x: T) -> T { x }\nmain = () => id(42)"),
        None,
    )
    .unwrap();
}

#[test]
fn e2e_generic_struct() {
    assert_eq!(pipeline::run(&temp_g("struct Container<T> { value: T }\nmain = () => { let c = Container { value: 42 }; c.value }"), None).unwrap(), 42);
}

#[test]
fn e2e_generic_edge() {
    let src = "struct Edge<T> { from: T, to: T }\nimpl<T> Edge<T> {\n    fn new(from: T, to: T) -> Edge<T> { Edge { from, to } }\n}\nfn main() -> i64 {\n    let e: Edge<i64> = Edge::new(0, 100)\n    let (from, to) = (e.from, e.to)\n    from - to\n}";
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), -100);
}

#[test]
fn e2e_mono_generic_fn_discovered_without_call_type_args() {
    let src = "fn id<T>(x: T) -> T { x }\nfn main() -> i64 { id(42) }";
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 42);
}

#[test]
fn e2e_mono_non_generic_param_before_generic() {
    let src = "fn wrap<T>(label: i64, value: T) -> T { value }\nfn main() -> i64 { wrap(0, 99) }";
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 99);
}

#[test]
fn e2e_mono_two_instantiations_same_fn() {
    let src = "fn id<T>(x: T) -> T { x }\nfn main() -> i64 { let a = id(42); let b = id(true); if b { a } else { 0 } }";
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 42);
}

#[test]
fn e2e_mono_generic_fn_with_two_type_params() {
    let src = "fn pair<A,B>(a: A, b: B) -> B { b }\nfn main() -> i64 { pair(1, 42) }";
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 42);
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
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 1);
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
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 42);
}

#[test]
fn e2e_generic_equality() {
    let src = r#"
fn eq<K>(a: K, b: K) -> bool { a == b }
main = () => {
    if eq(42, 42) { 1 } else { 0 }
}
"#;
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 1);
}

#[test]
fn e2e_generic_equality_false() {
    let src = r#"
fn eq<K>(a: K, b: K) -> bool { a == b }
main = () => {
    if eq(42, 99) { 1 } else { 0 }
}
"#;
    assert_eq!(pipeline::run(&temp_g(src), None).unwrap(), 0);
}

