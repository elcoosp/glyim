// test-mode: run-pass
// exit-code: 5

fn main() -> i32 {
    let x = 5;
    match x {
        n if n > 10 => 100,
        n if n > 3 => 5,
        _ => 0,
    }
}
