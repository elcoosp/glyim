// Nested `include!`: `middle.g` itself includes `inner.g`. Expansion is
// recursive and every spliced item shifts the `LocalDefId` numbering, so
// `main`'s id must be resolved from the *expanded* tree (a stale id from
// the unexpanded tree silently made the wrong function the entry point).
//@ mode: run-pass
//@ expect-exit: 6
//@ feature: LANG-MACRO-BUILTIN-INCLUDE, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
include!("middle.g");
fn main() -> i32 { middle() }
