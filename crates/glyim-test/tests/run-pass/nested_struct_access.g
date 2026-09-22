// test-mode: run-pass
// exit-code: 42

struct Inner { value: i32 }
struct Outer { inner: Inner }

fn main() -> i32 {
    let o = Outer { inner: Inner { value: 42 } };
    o.inner.value
}
