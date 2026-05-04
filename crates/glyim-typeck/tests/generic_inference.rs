use glyim_typeck::TypeChecker;
use glyim_parse::parse;
use glyim_hir::lower;
use glyim_hir::types::HirType;

fn typecheck_and_get_tc(source: &str) -> TypeChecker {
    let parse_out = parse(source);
    assert!(
        parse_out.errors.is_empty(),
        "parse errors: {:?}",
        parse_out.errors
    );
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let mut tc = TypeChecker::new(interner);
    tc.check(&hir).expect("type check should succeed");
    tc
}

#[test]
fn generic_call_id_infers_int() {
    let source = "fn id<T>(x: T) -> T { x }\nfn main() -> i64 { id(42) }";
    let tc = typecheck_and_get_tc(source);
    assert!(!tc.call_type_args.is_empty(), "call_type_args should not be empty");
    let type_args = tc.call_type_args.values().next().unwrap();
    assert_eq!(type_args.len(), 1, "id should have one type param");
    assert_eq!(type_args[0], HirType::Int, "T should be inferred as Int from arg 42");
}

#[test]
fn generic_struct_lit_annotation_infers_type_args() {
    let source = r#"
struct Container<T> { value: T }
main = () => {
    let c: Container<i64> = Container { value: 42 };
    c.value
}
"#;
    let tc = typecheck_and_get_tc(source);
    let found = tc.call_type_args.values().any(|args| args == &vec![HirType::Int]);
    assert!(found, "expected type args [Int] for Container, got {:?}", tc.call_type_args);
}

#[test]
fn method_call_on_generic_receiver_infers_type_args() {
    let source = r#"
struct Vec<T> { data: *mut u8, len: i64, cap: i64 }
impl<T> Vec<T> {
    fn new() -> Vec<T> { Vec { data: 0 as *mut u8, len: 0, cap: 0 } }
    fn push(mut self: Vec<T>, value: T) -> Vec<T> {
        self.len = self.len + 1; self
    }
}
fn main() -> i64 {
    let v: Vec<i64> = Vec::new();
    let v = v.push(10);
    v.len
}
"#;
    let tc = typecheck_and_get_tc(source);
    // We should have at least one call_type_args entry with [Int] (for push)
    let has_i64 = tc.call_type_args.values().any(|args| args == &vec![HirType::Int]);
    assert!(has_i64, "expected type args [Int] for Vec::push, got {:?}", tc.call_type_args);
}

#[test]
fn generic_call_with_multiple_params() {
    let source = r#"
fn pair<A, B>(a: A, b: B) -> B { b }
fn main() -> i64 { pair(1, 42) }
"#;
    let tc = typecheck_and_get_tc(source);
    let has_pair = tc.call_type_args.values().any(|args| args == &vec![HirType::Int, HirType::Int]);
    assert!(has_pair, "expected type args [Int, Int] for pair, got {:?}", tc.call_type_args);
}

#[test]
fn generic_call_zero_args_type_inference() {
    // Vec<T>::new() has zero arguments, but can infer type from annotation
    let source = r#"
struct Vec<T> { data: *mut u8, len: i64, cap: i64 }
impl<T> Vec<T> {
    fn new() -> Vec<T> { Vec { data: 0 as *mut u8, len: 0, cap: 0 } }
}
fn main() -> i64 {
    let v: Vec<i64> = Vec::new();
    v.len
}
"#;
    let tc = typecheck_and_get_tc(source);
    let has_i64 = tc.call_type_args.values().any(|args| args == &vec![HirType::Int]);
    assert!(has_i64, "expected type args [Int] for Vec::new(), got {:?}", tc.call_type_args);
}
