// ANTI-REGRESSION / ANTI-MISCOMPILATION: `let` inside a nested block
// must not leak into the enclosing scope.
//
// Two stacked bugs:
//   1. `Expr::Block` never called `env.enter_scope`/`leave_scope`, so
//      `let x = 5; { let x = 10; } x` resolved the trailing `x` to the
//      inner binding (10, not 5) — a *silent wrong-value* bug.
//   2. `LocalEnv::leave_scope` popped the name-map entry for every
//      binding in `[base..vars.len())` unconditionally. Two nested
//      scopes can share the same `base` (no bindings between them); the
//      outer `leave_scope` then popped the outer binding of the same
//      name. This was latent until (1) started triggering it, and
//      surfaced as `unresolved name \`fut\`` in the async desugar's
//      nested state-machine arms. Fixed by only popping when the
//      binding is on top of the name-map stack.
//
// Distinct primes across the shadow and the outer read make a leak
// produce a distinctly wrong exit code.
// test-mode: run-pass
// exit-code: 40
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let x = 5;
    let y = { let x = 20; x };   // inner x = 20
    let z = {
        let x = 15;
        { let x = 99; }          // doubly-nested: doesn't escape either
        x                        // 15
    };
    // x + y + z = 5 + 20 + 15 = 40
    x + y + z
}
