//@ ignore
// requires full typeck/frontend (S03): enum `Option` / `Some` variant resolution
//@ feature: LANG-MATCH-GUARDS, TOOL-TEST-COMPILE-PASS
//@ backends: gam
fn main(x: Option<i32>) -> i32 {
    match x {
        Some(y) if y > 0 => y,
        _ => 0
    }
}
