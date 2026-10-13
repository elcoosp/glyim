// test-mode: compile-fail
fn make<T>() -> T { }
fn main() -> i32 {
    make() //~ ERROR mismatched types
}
