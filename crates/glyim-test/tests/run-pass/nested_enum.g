// test-mode: run-pass
// exit-code: 5

enum Inner { A(i32), B }
struct Wrapper { inner: Inner, label: i32 }

fn main() -> i32 {
    let w = Wrapper { inner: Inner::A(5), label: 99 };
    match w.inner {
        Inner::A(v) => v,
        Inner::B => 0,
    }
}
