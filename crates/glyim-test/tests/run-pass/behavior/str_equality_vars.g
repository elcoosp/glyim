// ANTI-REGRESSION: string equality across `let` bindings, including the
// empty-string path.
//
// The empty-string branch exercises `glyim_str_eq`'s `l_len == 0` early
// return: the byte pointers may be null/dangling, so a wrong
// implementation that dereferences before checking length segfaults.
//@ mode: run-pass
//@ expect-exit: 19
//@ compile-flags: --with-stdlib
//@ needs-llvm
//@ feature: LANG-TY-STR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let empty_a = "";
    let empty_b = "";
    let non_empty = "x";
    let s = "hello";
    let t = "hello";

    let x = if empty_a == empty_b { 7 } else { 0 };
    let y = if empty_a == non_empty { 0 } else { 5 };
    let z = if s == t { 7 } else { 0 };
    x + y + z
}
