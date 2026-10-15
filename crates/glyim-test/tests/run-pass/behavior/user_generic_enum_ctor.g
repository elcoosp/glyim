// User-declared generic enum data-variant ctor, inferred payload.
// test-mode: run-pass
// exit-code: 7
// compile-flags: --with-stdlib
enum MyOpt<T> { S(T), N }
fn main() -> i32 {
    let a = MyOpt::S(7);
    match a {
        MyOpt::S(x) => x,
        MyOpt::N => 0,
    }
}
