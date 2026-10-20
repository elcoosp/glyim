// Same as const_enum_variant but through `static`, which lowers via
// `MirConstKind::StaticRef` wrapping the `Variant` initializer.
// test-mode: run-pass
// exit-code: 53
// compile-flags: --with-stdlib
enum C { A, B }
static K: C = C::B;
fn main() -> i32 { match K { C::A => 1, C::B => 53 } }
