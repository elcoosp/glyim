// test-mode: run-pass
// exit-code: 10

fn main() -> i32 {
    let mut i = 0;
    let mut sum = 0;
    while i < 5 {
        sum = sum + i;
        i = i + 1;
    }
    sum
}
