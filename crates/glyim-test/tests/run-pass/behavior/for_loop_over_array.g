// KNOWN BUG — for-loop over a fixed-size array hangs at runtime.
//
// `for x in [1, 2, 3]` should bind `x: i32` by value (Rust semantics for
// by-value array iteration). Typeck correctly sets `next = None` and
// `item_ty = T` for array iterables, routing around the `Iterator`
// trait (`impl Iterator for &[T]` yields `&T`, an 8-byte pointer that
// would corrupt a 4-byte `x` slot).
//
// The lowering's array-index path is incomplete: an initial attempt
// emitted the loop but `idx`/`len` locals were dropped because
// `self.current_block` was `None` after the preceding `terminate(Goto)`.
// After adding `self.current_block = Some(header_bb)`, the binary
// compiled and ran but *hung* — the loop condition or increment path
// has its own bug. Reverted; tracked here for a dedicated session.
//
// The correct fix is a careful single-step IR comparison of an emitted
// array loop against the equivalent `while idx < len { ...; idx += 1 }`
// built by hand — the MIR is structurally identical, so the hang is in
// codegen (most likely the `Lt`/`Add` on `usize` locals, or the
// `SwitchTargets::if_switch` order).
//
// test-mode: run-pass
// ignore
// exit-code: 15
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a = [1, 2, 3, 4, 5];
    let mut s = 0;
    for x in a { s = s + x; }
    s
}
