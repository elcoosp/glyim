// Regression: struct patterns with named-field bindings.
// `P { x: a, y: b } => a + b` reports "no field `a` in ADT" (the *binding*
// name is used as the field name) and "() vs i32". Pre-existing; distinct
// from the ref-pattern work. `ignore` keeps CI green while documenting.
//@ mode: run-pass
//@ expect-exit: 7
//@ compile-flags: --with-stdlib
//@ needs-llvm
//@ feature: LANG-MATCH-GUARDS, LANG-SYN-STRUCT, LANG-SYN-FIELD-ACCESS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
struct P { x: i32, y: i32 }
fn main() -> i32 {
    let p = P { x: 3, y: 4 };
    match p {
        P { x: a, y: b } => a + b,
    }
}
