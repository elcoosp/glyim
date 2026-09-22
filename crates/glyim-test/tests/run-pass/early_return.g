// test-mode: run-pass
// exit-code: 7

fn classify(n: i32) -> i32 {
    if n < 0 { return 0; }
    if n > 5 { return 7; }
    n
}

fn main() -> i32 { classify(10) }
