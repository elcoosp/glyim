//@ feature: LANG-SYN-FN-DEF, TOOL-TEST-NO-ICE
//@ backends: gam
struct A { b: B }
struct B { a: A }
fn main() {}
