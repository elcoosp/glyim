//@ mode: compile-pass
// V16-T01: Option::unwrap works
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let val: Option<i32> = Option::Some(42);
    let unwrapped = val.unwrap();
    assert_eq!(unwrapped, 42);
}
