// Two pointers to different locals must be unequal — exercises the
// `ne` arm's pointer path (same lowering as `eq`, `NE` predicate).
// test-mode: run-pass
// exit-code: 1
// compile-flags: --with-stdlib
fn main() -> i32 {
    let x = 7;
    let y = 7;
    let p1 = &x as *const i32;
    let p2 = &y as *const i32;
    if p1 != p2 { 1 } else { 0 }
}
