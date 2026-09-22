// test-mode: run-pass
// exit-code: 3
fn main() -> i32 {
    let mut i = 0;
    loop {
        if i == 3 { break; }
        i = i + 1;
    }
    i
}
