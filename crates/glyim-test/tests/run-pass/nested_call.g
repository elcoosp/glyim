// test-mode: run-pass
// exit-code: 10

fn double(x: i32) -> i32 { x + x }
fn add(a: i32, b: i32) -> i32 { a + b }

fn main() -> i32 {
    add(double(3), 4)
}
