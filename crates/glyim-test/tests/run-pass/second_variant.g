// The other variant must be selected by the discriminant.
// test-mode: run-pass
// exit-code: 7

enum E { A(i32), B(i32) }

fn main() -> i32 {
    let e = E::B(7);
    match e {
        E::A(_) => 0,
        E::B(v) => v,
    }
}
