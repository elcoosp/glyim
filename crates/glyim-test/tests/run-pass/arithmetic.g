// test-mode: run-pass
// exit-code: 4

fn main() -> i32 {
    let a = 5;
    let b = 3;
    (a + b) - (a - b) - (a - b)
}
