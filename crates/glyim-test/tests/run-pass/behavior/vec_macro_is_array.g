// Regression for §3.7 sub-issue (2), fixed in commit 22eb0ea2: `vec![...]`
// was expanding to `[[...]]` (double-wrapped array) which silently
// miscompiled `.len()` to 1 and gave `for x in vec![...]` an unresolved
// element type. Now expands correctly to `[...]` (a single array).
//
// Design note: `vec![...]` is an array, NOT a `Vec` — see §3.7's design
// note. `.push()` / `.as_slice()` still need the `Vec::new()` route.
//@ mode: run-pass
//@ expect-exit: 3
//@ feature: LANG-TY-ARRAY, LANG-BUILTIN-VEC, STD-ALLOC-VEC, LANG-MACRO-RULES, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let v = vec![1, 2, 3];
    v.len() as i32
}
