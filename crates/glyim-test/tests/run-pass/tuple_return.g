// test-mode: run-pass
// exit-code: 22

fn pair() -> (i32, i32) { (11, 22) }

fn main() -> i32 {
    let (a, b) = pair();
    b
}
