// Regression: `ref` binding on a named struct field.
// Same root as match_struct_named_fields.g.
//@ mode: run-pass
//@ expect-exit: 12
//@ compile-flags: --with-stdlib
//@ needs-llvm
//@ feature: LANG-MATCH-GUARDS, LANG-SYN-STRUCT, LANG-SYN-FIELD-ACCESS, LANG-TY-REF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
struct P { x: i32, y: i32 }
fn main() -> i32 {
    let p = P { x: 3, y: 4 };
    match p {
        P { x: ref a, y: ref b } => *a * *b,
    }
}
