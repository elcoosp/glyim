// KNOWN FAILING — `ref` binding on a named struct field.
// Same root as match_struct_named_fields.g.
// test-mode: run-pass
// ignore
// exit-code: 12
// compile-flags: --with-stdlib
// needs-llvm
struct P { x: i32, y: i32 }
fn main() -> i32 {
    let p = P { x: 3, y: 4 };
    match p {
        P { x: ref a, y: ref b } => *a * *b,
    }
}
