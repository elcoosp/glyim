//@ mode: ui
//@ feature: COMPILER-FRONT-DEFTABLE, TOOL-TEST-UI
//@ backends: gam
struct Foo;
fn main() {
    let f = Foo;
    f.bar();
}
