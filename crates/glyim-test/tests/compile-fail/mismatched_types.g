// test-mode: compile-fail
fn main() -> i32 {
    let x: i32 = 1;
    let y: bool = true;
    x + y //~ ERROR mismatched types
}
