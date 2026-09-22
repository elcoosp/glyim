// test-mode: run-pass
// exit-code: 30

struct Pair { a: i32, b: i32 }

fn main() -> i32 {
    let p = Pair { a: 10, b: 20 };
    p.a + p.b
}
