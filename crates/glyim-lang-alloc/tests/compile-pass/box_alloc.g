//@ mode: compile-pass
// V17-T03: Box allocation and deref works
//@ feature: LANG-BUILTIN-BOX, STD-ALLOC-BOX, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let x = Box::new(42);
    assert_eq!(*x, 42);
}
