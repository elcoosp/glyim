//@ mode: compile-fail
//@ feature: COMPILER-FRONT-DEFTABLE, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
struct Foo;
fn main() {
    let f = Foo;
    f.bar(); //~ ERROR no method `bar` found
}
