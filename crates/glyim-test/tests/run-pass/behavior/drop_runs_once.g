// A user `Drop` impl must run exactly once per value. Enabling drop
// execution exposed that the aggregate temp and the `let` binding it is
// moved into were *both* dropped — a double-free for any owning field.
//
// The value is dropped at `work`'s return (scope-exit drops are emitted per
// *function*, not per inner block — a separate, pre-existing gap).
// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
static mut COUNT: i32 = 0;
struct G { v: i32 }
impl Drop for G {
    fn drop(&mut self) { unsafe { COUNT = COUNT + 1; } }
}
fn work() { let _g = G { v: 1 }; }
fn main() -> i32 {
    work();
    unsafe { COUNT + 4 }
}
