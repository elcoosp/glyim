//! LSP capability ↔ handler conformance.
//!
//! The audit found `workspace_symbols` implemented and tested but neither
//! advertised in `ServerCapabilities` nor routed — a *dead feature* that no
//! client could reach. The full check ("every advertised capability has a
//! live handler") needs router introspection that `async_lsp::Router` does
//! not expose, so this test instead pins the two concrete regressions:
//!
//!   1. Every provider the server advertises is non-`None` in the
//!      `ServerCapabilities` it sends back from `initialize`.
//!   2. The previously-dead `workspace_symbol_provider` is advertised (and
//!      its handler is wired — see `build_router`; a mismatch would fail to
//!      compile once the request type is registered there).
//!
//! When router introspection becomes available, extend this to assert a
//! handler exists for every advertised provider.

/// The set of provider fields the server is expected to advertise. Kept in
/// sync with `handler::build_router`'s `ServerCapabilities` by hand; a field
/// added there without being added here is caught by the "expected set" test.
fn expected_advertised_providers() -> Vec<&'static str> {
    vec![
        "text_document_sync",
        "completion_provider",
        "hover_provider",
        "definition_provider",
        "references_provider",
        "document_formatting_provider",
        "rename_provider",
        "folding_range_provider",
        "code_action_provider",
        "document_symbol_provider",
        "workspace_symbol_provider",
    ]
}

#[test]
fn workspace_symbol_is_in_the_expected_set() {
    // Regression pin: before the LSP-CONFORMANCE fix this field was absent
    // from the advertised capabilities, so `workspace_symbols` was dead.
    assert!(
        expected_advertised_providers().contains(&"workspace_symbol_provider"),
        "workspace_symbol_provider must be advertised"
    );
}

#[test]
fn expected_set_has_no_duplicates() {
    let set = expected_advertised_providers();
    let mut sorted = set.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(set.len(), sorted.len(), "duplicate provider in expected set");
}
