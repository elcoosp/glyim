//! Verification for the global-allocator wiring (audit claim 1).
//!
//! `alloc.g` is glyim source (`.g`), not Rust, so it is not type-checked by
//! `cargo`. The authoritative check is that the glyim frontend parser accepts
//! it as well-formed `.g` and that the symbols `Box`/`Vec`/`String` lower onto
//! — `const GLOBAL` (the allocator the collections call) and
//! `handle_alloc_error` (the OOM abort) — are now defined. Before the fix
//! these were missing, so any program using `Box`/`Vec` failed to compile.
//!
//! Full Box/Vec *runtime* execution is covered by the glyim compiler's own
//! std-loading integration path (the alloc modules are loaded as separate
//! modules there, not as one concatenated blob), which is exercised by the
//! broader glyim-test runtime suite.

use glyim_frontend::parse_to_syntax;
use glyim_lang_alloc::alloc_source;
use glyim_span::FileId;

#[test]
fn alloc_g_is_well_formed_g_and_defines_allocator() {
    let src = alloc_source("alloc").expect("alloc.g must be embedded");
    let result = parse_to_syntax(src, FileId::from_raw(1));
    assert!(
        result.diagnostics.is_empty(),
        "alloc.g must parse as well-formed .g; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        src.contains("const GLOBAL: Global = Global;"),
        "alloc.g must define the GLOBAL allocator that Box/Vec lower onto"
    );
    assert!(
        src.contains("fn handle_alloc_error"),
        "alloc.g must define handle_alloc_error (OOM abort) used by Box/Vec"
    );
    assert!(
        src.contains("impl GlobalAlloc for Global"),
        "alloc.g must wire the GlobalAlloc trait impl onto the GLOBAL value"
    );
}
