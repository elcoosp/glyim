// User-declared generic enum data-variant ctor, inferred payload.
//@ mode: run-pass
//@ expect-exit: 7
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-ENUM, LANG-SYN-TUPLE-STRUCT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
enum MyOpt<T> { S(T), N }
fn main() -> i32 {
    let a = MyOpt::S(7);
    match a {
        MyOpt::S(x) => x,
        MyOpt::N => 0,
    }
}
