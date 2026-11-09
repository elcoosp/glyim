// FnDef -> FnPtr coercion of a generic fn item under explicit
// instantiation (`id::<i32>`). The registered sig of `id<T>` carries
// `Param(0)`; the FnDef's own substitution `[i32]` is substituted into
// that sig at unify time so the FnPtr signature matches structurally.
//@ mode: run-pass
//@ expect-exit: 67
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-RAW-PTR, STD-CORE-PTR, LANG-TY-FN-PTR, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn id<T>(x: T) -> T { x }
fn main() -> i32 {
    let g: fn(i32) -> i32 = id::<i32>;
    g(67)
}
