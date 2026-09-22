// test-mode: run-pass
// exit-code: 6

fn step1(x: i32) -> i32 { x + 1 }
fn step2(x: i32) -> i32 { step1(x) * 2 }
fn step3(x: i32) -> i32 { step2(x) + step1(x) }

fn main() -> i32 {
    step3(1)
}
