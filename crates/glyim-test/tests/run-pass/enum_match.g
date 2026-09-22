// A `match` on a data-carrying enum, returning its payload.
//
// Regression: this shape once returned 0 because `constant_prop` const-folded
// `E::A(42)` into a `MirConstKind::Aggregate` that dropped the variant index,
// so codegen wrote 42 into the tag byte and the `match` read the tag as the
// payload.
// test-mode: run-pass
// exit-code: 42

enum E { A(i32), B }

fn main() -> i32 {
    let e = E::A(42);
    match e {
        E::A(v) => v,
        E::B => 0,
    }
}
