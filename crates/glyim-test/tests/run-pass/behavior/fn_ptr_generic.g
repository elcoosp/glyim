// FnDef -> FnPtr coercion of a generic fn item under explicit
// instantiation (`id::<i32>`). The registered sig of `id<T>` carries
// `Param(0)`; the FnDef's own substitution `[i32]` is substituted into
// that sig at unify time so the FnPtr signature matches structurally.
// test-mode: run-pass
// exit-code: 67
// compile-flags: --with-stdlib
fn id<T>(x: T) -> T { x }
fn main() -> i32 {
    let g: fn(i32) -> i32 = id::<i32>;
    g(67)
}
