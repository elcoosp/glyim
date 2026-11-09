// Same as const_enum_variant but through `static`, which lowers via
// `MirConstKind::StaticRef` wrapping the `Variant` initializer.
//@ mode: run-pass
//@ expect-exit: 53
//@ compile-flags: --with-stdlib
//@ feature: LANG-SYN-ENUM, LANG-SCOPE-STATIC-ITEM, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
enum C { A, B }
static K: C = C::B;
fn main() -> i32 { match K { C::A => 1, C::B => 53 } }
