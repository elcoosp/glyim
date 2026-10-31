// §3.5.31(a): bare numeric-literal receivers must resolve primitive methods.
//
// Before the fix, `100.saturating_mul(2)` failed with [T0001] no method —
// the primitive lookup at resolve_method_call checked step_ty == Int(_)
// directly, and for a literal the type is Infer(Int(_)), missing the
// check entirely. Fix: resolve step_ty via resolve_ty_shallow first.
//
// checked_* on i32/i64 is not covered here: that family was never
// registered for signed types (see §3.5.32). The saturating and wrapping
// families both prove the literal-receiver fix is family-agnostic.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    // Literal receiver: saturating + wrapping
    if 100.saturating_mul(2) != 200 { return 1; }
    if 100.wrapping_mul(2) != 200 { return 2; }
    // Typed receiver still works
    let a: i32 = 100;
    if a.saturating_mul(2) != 200 { return 3; }
    // u32 literal with suffix wraps at UINT_MAX
    let ua: u32 = 4294967295;
    if ua.saturating_add(1) != 4294967295 { return 4; }
    // Non-literal untyped expression also defaults correctly
    let r = if true { 100 } else { 200 };
    if r.saturating_mul(2) != 200 { return 5; }
    0
}
