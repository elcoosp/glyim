//@ mode: compile-pass
// V17: Vec::pop returns Option
//@ feature: LANG-BUILTIN-VEC, STD-ALLOC-VEC, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
    let last = v.pop();
    assert_eq!(last, Option::Some(2));
    assert_eq!(v.len(), 1);
    let none = v.pop();
    assert_eq!(none, Option::Some(1));
    let empty = v.pop();
    assert_eq!(empty, Option::None);
}
