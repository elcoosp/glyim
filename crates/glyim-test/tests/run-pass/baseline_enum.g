// test-mode: run-pass
// exit-code: 42
enum E { A(i32), B }
fn main() -> i32 {
    let e = E::A(42);
    match e { E::A(v) => v, E::B => 0 }
}
