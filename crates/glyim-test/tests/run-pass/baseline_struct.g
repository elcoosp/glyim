// test-mode: run-pass
// exit-code: 20
struct P { x: i32, y: i32 }
fn main() -> i32 {
    let p = P { x: 10, y: 20 };
    p.y
}
