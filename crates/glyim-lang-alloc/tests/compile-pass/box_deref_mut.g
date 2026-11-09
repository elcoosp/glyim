//@ mode: compile-pass
// V17: Box deref_mut allows mutation
//@ feature: LANG-SYN-MUT, LANG-BUILTIN-BOX, STD-ALLOC-BOX, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let mut x = Box::new(10);
    *x += 5;
    assert_eq!(*x, 15);
}
