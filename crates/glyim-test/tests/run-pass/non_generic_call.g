// A non-generic function must be treated as non-generic.
//
// Regression: the `Call` arm once built the callee's `FnDef` substs from the
// formal parameter list, so `add_one(x: i32)` got a spurious `substs = [i32]`.
// test-mode: run-pass
// exit-code: 42

fn add_one(x: i32) -> i32 { x + 1 }

fn main() -> i32 {
    add_one(41)
}
