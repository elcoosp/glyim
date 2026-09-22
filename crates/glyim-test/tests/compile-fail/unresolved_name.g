// test-mode: compile-fail
fn main() -> i32 {
    undefined_symbol //~ ERROR unresolved name
}
