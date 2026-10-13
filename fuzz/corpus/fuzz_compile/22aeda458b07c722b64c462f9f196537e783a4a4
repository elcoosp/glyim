// A struct with two fields is not a scalar newtype and cannot be cast to int.
//
// Single-field newtype structs (`ThreadId(u64)`) ARE castable by design — the
// stdlib relies on it — so this uses a two-field struct to trigger the
// rejection.
// test-mode: compile-fail
struct Point { x: i32, y: i32 }
fn main() -> i32 {
    let p = Point { x: 1, y: 2 };
    let n = p as i32; //~ ERROR invalid cast
    n
}
