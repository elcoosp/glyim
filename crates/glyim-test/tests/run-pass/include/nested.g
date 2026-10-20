// Nested `include!`: `middle.g` itself includes `inner.g`. Expansion is
// recursive and every spliced item shifts the `LocalDefId` numbering, so
// `main`'s id must be resolved from the *expanded* tree (a stale id from
// the unexpanded tree silently made the wrong function the entry point).
// test-mode: run-pass
// exit-code: 6
include!("middle.g");
fn main() -> i32 { middle() }
