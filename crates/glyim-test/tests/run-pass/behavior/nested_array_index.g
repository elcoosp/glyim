// ANTI-REGRESSION / ANTI-MISCOMPILATION: 2D array indexing
// `m[0][0]` must work.
//
// `Expr::Index`'s `get_index_info` classified the base type without
// resolving it through the inference table. For a nested array
// `[[1, 2], [3, 4]]`, the outer `Array(_, _)`'s element type is a
// fresh `Infer(?T)` — it *was* bound to `[i32; 2]`, but
// `resolve_ty_shallow` hadn't been called. `m[0]` therefore had type
// `Infer(?T)`, and `m[0][0]` saw `TyKind::Infer` instead of
// `TyKind::Array` and reported "indexing operation requires array,
// slice, Vec, or str type".
//
// Distinct values per cell make any wrong-offset read produce a
// distinctly wrong exit code.
// test-mode: run-pass
// exit-code: 10
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let m = [[1, 2], [3, 4]];
    // 1 + 2 + 3 + 4 = 10
    m[0][0] + m[0][1] + m[1][0] + m[1][1]
}
