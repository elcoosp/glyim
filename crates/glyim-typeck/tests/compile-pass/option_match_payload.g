// Regression: matching an `Option<T>` and binding/using the payload used to
// ICE in codegen (`IndexVec::get: index out of bounds` at
// `glyim-core/src/arena.rs:173`, reached from
// `glyim-codegen-llvm/src/lower.rs:544`). The bare `Field(..)` projection
// over a multi-variant enum payload read `layout.fields.offsets[1]` with an
// `IndexVec::get` whose `debug_assert!` fired when the offsets table had
// fewer than two entries. Codegen now uses a slice `get` for the optional
// tag-prefix offset.
fn unwrap_or_zero(o: Option<u64>) -> u64 {
    match o {
        Option::Some(v) => v,
        Option::None => 0,
    }
}

fn main() {
    let a = unwrap_or_zero(Option::Some(7));
    let b = unwrap_or_zero(Option::None);
    let _ = (a, b);
}
