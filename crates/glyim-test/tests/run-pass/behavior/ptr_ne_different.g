// Two pointers to different locals must be unequal — exercises the
// `ne` arm's pointer path (same lowering as `eq`, `NE` predicate).
//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-RAW-PTR, STD-CORE-PTR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let x = 7;
    let y = 7;
    let p1 = &x as *const i32;
    let p2 = &y as *const i32;
    if p1 != p2 { 1 } else { 0 }
}
