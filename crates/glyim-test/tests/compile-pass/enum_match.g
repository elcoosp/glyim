// test-mode: compile-pass
enum Pair { Both(i32, i32), Neither }
fn main() -> i32 {
    let p = Pair::Both(1, 2);
    match p {
        Pair::Both(a, b) => a + b,
        Pair::Neither => 0,
    }
}
