//@ mode: compile-pass
// V17: String::default() works
//@ feature: LANG-BUILTIN-STRING, STD-ALLOC-STRING, LANG-TRAIT-DEFAULT-METHODS, STD-CORE-DEFAULT, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let s: String = Default::default();
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
    assert_eq!(s.as_str(), "");
}
