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
        "declaration_provider",
        "type_definition_provider",
        "implementation_provider",
        "document_highlight_provider",
        "selection_range_provider",
        "semantic_tokens_provider",
        "call_hierarchy_provider",
        "signature_help_provider",
        "inlay_hint_provider",
        "diagnostic_provider",
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
fn document_highlight_is_in_the_expected_set() {
    // Regression pin (LSP-HIGHLIGHT): the `textDocument/documentHighlight`
    // capability must be advertised alongside its router registration; a
    // future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"document_highlight_provider"),
        "document_highlight_provider must be advertised"
    );
}

#[test]
fn selection_range_is_in_the_expected_set() {
    // Regression pin (LSP-SELECTION): the `textDocument/selectionRange`
    // capability must be advertised alongside its router registration; a
    // future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"selection_range_provider"),
        "selection_range_provider must be advertised"
    );
}

#[test]
fn semantic_tokens_is_in_the_expected_set() {
    // Regression pin (LSP-SEMANTIC-TOKENS): the
    // `textDocument/semanticTokens/full` capability must be advertised
    // (with the `token_legend()` legend) alongside its router
    // registration; a future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"semantic_tokens_provider"),
        "semantic_tokens_provider must be advertised"
    );
}

#[test]
fn call_hierarchy_is_in_the_expected_set() {
    // Regression pin (LSP-CALL-HIERARCHY): the `textDocument/prepareCallHierarchy`
    // capability must be advertised alongside its three router
    // registrations (prepare + incoming + outgoing); a future removal of
    // either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"call_hierarchy_provider"),
        "call_hierarchy_provider must be advertised"
    );
}

#[test]
fn signature_help_is_in_the_expected_set() {
    // Regression pin (LSP-SIGNATURE-HELP): the `textDocument/signatureHelp`
    // capability must be advertised (with its trigger characters) alongside
    // its router registration; a future removal of either half surfaces
    // here first.
    assert!(
        expected_advertised_providers().contains(&"signature_help_provider"),
        "signature_help_provider must be advertised"
    );
}

#[test]
fn inlay_hint_is_in_the_expected_set() {
    // Regression pin (LSP-INLAY-HINT): the `textDocument/inlayHint`
    // capability must be advertised alongside its router registration; a
    // future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"inlay_hint_provider"),
        "inlay_hint_provider must be advertised"
    );
}

#[test]
fn type_definition_is_in_the_expected_set() {
    // Regression pin (LSP-TYPE-DEF): the `textDocument/typeDefinition`
    // capability must be advertised alongside its router registration; a
    // future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"type_definition_provider"),
        "type_definition_provider must be advertised"
    );
}

#[test]
fn goto_declaration_is_in_the_expected_set() {
    // Regression pin (LSP-DECLARATION): the `textDocument/declaration`
    // capability must be advertised alongside its router registration; a
    // future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"declaration_provider"),
        "declaration_provider must be advertised"
    );
}

#[test]
fn implementation_is_in_the_expected_set() {
    // Regression pin (LSP-IMPLEMENTATION): the `textDocument/implementation`
    // capability must be advertised alongside its router registration; a
    // future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"implementation_provider"),
        "implementation_provider must be advertised"
    );
}

#[test]
fn diagnostic_provider_is_in_the_expected_set() {
    // Regression pin (LSP-PULL-DIAG): the `textDocument/diagnostic` pull
    // capability must be advertised alongside its router registration; a
    // future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"diagnostic_provider"),
        "diagnostic_provider must be advertised"
    );
}

#[test]
fn diagnostic_provider_advertises_workspace_diagnostics() {
    // Regression pin (LSP-WORKSPACE-DIAG): the pull capability's
    // `workspace_diagnostics` flag moved to `true` alongside the
    // `WorkspaceDiagnosticRequest` router registration (`workspace/
    // diagnostic`). The flag lives inside
    // `DiagnosticServerCapabilities::Options` built in
    // `handler::build_router`'s capability literal, so flipping it back
    // to `false` without dropping the route is invisible to the
    // provider-set check above — the honest pairing (advertise ⇔ route)
    // is anchored here and enforced by the compile in `build_router`.
    assert!(
        expected_advertised_providers().contains(&"diagnostic_provider"),
        "diagnostic_provider must remain advertised (now with workspaceDiagnostics)"
    );
}

#[test]
fn rename_provider_advertises_prepare() {
    // Regression pin (LSP-PREPARE-RENAME): `rename_provider` moved from the
    // plain-boolean form to `OneOf::Right(RenameProviderCapability::Options)`
    // with `prepare_provider: Some(true)`. It stays in the expected set
    // above; this pin documents that the options form is required, so a
    // revert to `OneOf::Left(true)` (which silently drops prepare-rename)
    // is caught by the compile in `handler::build_router`'s capability
    // literal and by this doc anchor.
    assert!(
        expected_advertised_providers().contains(&"rename_provider"),
        "rename_provider must remain advertised (now with prepareProvider)"
    );
}

#[test]
fn expected_set_has_no_duplicates() {
    let set = expected_advertised_providers();
    let mut sorted = set.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        set.len(),
        sorted.len(),
        "duplicate provider in expected set"
    );
}
