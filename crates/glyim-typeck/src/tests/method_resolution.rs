use glyim_hir::lower;
use glyim_parse::parse;
use crate::TypeChecker;

fn typecheck_source(source: &str) -> TypeChecker {
    let parse_out = parse(source);
    assert!(
        parse_out.errors.is_empty(),
        "parse errors: {:?}",
        parse_out.errors
    );
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let mut tc = TypeChecker::new(interner);
    let _ = tc.check(&hir);
    tc
}

#[test]
fn impl_method_is_registered() {
    let mut tc = typecheck_source(
        "struct Point { x, y }\nimpl Point {\n    fn zero() -> Point { Point { x: 0, y: 0 } }\n}\nfn main() -> i64 { 0 }",
    );
    let point_sym = tc.interner.intern("Point");
    assert!(
        tc.impl_methods.contains_key(&point_sym),
        "impl methods for Point should be registered"
    );
}

#[test]
fn generic_impl_method_is_registered_in_impl_methods() {
    let mut tc = typecheck_source(
        "struct Vec<T> { data: *mut u8, len: i64, cap: i64 }\nimpl<T> Vec<T> {\n    fn new() -> Vec<T> { Vec { data: 0 as *mut u8, len: 0, cap: 0 } }\n}\nfn main() -> i64 { 0 }",
    );
    let vec_sym = tc.interner.intern("Vec");
    assert!(
        tc.impl_methods.contains_key(&vec_sym),
        "Vec impl should be registered"
    );
}

#[test]
fn extern_fn_is_registered() {
    let mut tc = typecheck_source(
        "extern {\n    fn write(fd: i64, buf: *const u8, len: i64) -> i64;\n}\nfn main() -> i64 { 0 }",
    );
    let write_sym = tc.interner.intern("write");
    assert!(
        tc.extern_fns.contains_key(&write_sym),
        "write should be in extern_fns"
    );
}

#[test]
fn multiple_impl_methods_are_registered() {
    let mut tc = typecheck_source(
        "struct Counter { val: i64 }\nimpl Counter {\n    fn inc(mut self: Counter) -> Counter { self.val = self.val + 1; self }\n    fn dec(mut self: Counter) -> Counter { self.val = self.val - 1; self }\n}\nfn main() -> i64 { 0 }",
    );
    let counter_sym = tc.interner.intern("Counter");
    let methods = tc
        .impl_methods
        .get(&counter_sym)
        .expect("Counter should have methods");
    assert!(
        methods
            .iter()
            .any(|m| tc.interner.resolve(m.name) == "Counter_inc"),
        "inc method missing"
    );
    assert!(
        methods
            .iter()
            .any(|m| tc.interner.resolve(m.name) == "Counter_dec"),
        "dec method missing"
    );
}
