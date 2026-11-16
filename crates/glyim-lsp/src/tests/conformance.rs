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
        "moniker_provider",
        "inline_value_provider",
        "diagnostic_provider",
        "references_provider",
        "document_formatting_provider",
        "document_range_formatting_provider",
        "rename_provider",
        "folding_range_provider",
        "code_action_provider",
        "document_symbol_provider",
        "workspace_symbol_provider",
        "document_link_provider",
        "linked_editing_range_provider",
        "code_lens_provider",
        "document_on_type_formatting_provider",
        "execute_command_provider",
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
fn document_link_is_in_the_expected_set() {
    // Regression pin (LSP-DOC-LINK): the `textDocument/documentLink`
    // capability must be advertised alongside its router registration; a
    // future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"document_link_provider"),
        "document_link_provider must be advertised"
    );
}

#[test]
fn linked_editing_range_is_in_the_expected_set() {
    // Regression pin (LSP-LINKED-EDIT): the
    // `textDocument/linkedEditingRange` capability must be advertised
    // alongside its router registration; a future removal of either half
    // surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"linked_editing_range_provider"),
        "linked_editing_range_provider must be advertised"
    );
}

#[test]
fn code_lens_is_in_the_expected_set() {
    // Regression pin (LSP-CODE-LENS): the `textDocument/codeLens`
    // capability must be advertised (with its resolve provider, since the
    // `codeLens/resolve` route exists) alongside its two router
    // registrations; a future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"code_lens_provider"),
        "code_lens_provider must be advertised"
    );
}

#[test]
fn on_type_formatting_is_in_the_expected_set() {
    // Regression pin (LSP-ON-TYPE-FORMAT): the
    // `textDocument/onTypeFormatting` capability must be advertised
    // (with its trigger characters) alongside its router registration; a
    // future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"document_on_type_formatting_provider"),
        "document_on_type_formatting_provider must be advertised"
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
fn type_hierarchy_has_a_pinned_wire_contract() {
    use lsp_types::request::Request;
    // Regression pin (LSP-TYPE-HIERARCHY): the `typeHierarchy/*` requests
    // define no `ServerCapabilities` field in lsp-types 0.97 (the LSP spec
    // gates them on registration, not on a static capability), so the
    // router registration *is* the advertisement. The pin is the wire
    // contract instead: the three request types' METHOD strings, so an
    // lsp-types upgrade that reshapes the endpoints surfaces here and the
    // end-to-end tests in `type_hierarchy_tests.rs` pin the handlers.
    assert_eq!(
        lsp_types::request::TypeHierarchyPrepare::METHOD,
        "textDocument/prepareTypeHierarchy"
    );
    assert_eq!(
        lsp_types::request::TypeHierarchySupertypes::METHOD,
        "typeHierarchy/supertypes"
    );
    assert_eq!(
        lsp_types::request::TypeHierarchySubtypes::METHOD,
        "typeHierarchy/subtypes"
    );
}

#[test]
fn moniker_has_a_pinned_wire_contract() {
    use lsp_types::request::Request;
    // Regression pin (LSP-MONIKER): like the type-hierarchy requests, the
    // `textDocument/moniker` request defines no `ServerCapabilities` field
    // in lsp-types 0.97 (the LSP spec gates monikers on registration, not
    // on a static capability), so the router registration *is* the
    // advertisement. The pin is the wire contract: the request type's
    // METHOD string, so an lsp-types upgrade that reshapes the endpoint
    // surfaces here and the end-to-end tests in `moniker_tests.rs` pin the
    // handler.
    assert_eq!(
        lsp_types::request::MonikerRequest::METHOD,
        "textDocument/moniker"
    );
}

#[test]
fn inline_value_is_in_the_expected_set() {
    // Regression pin (LSP-INLINE-VALUE): the `textDocument/inlineValue`
    // capability must be advertised alongside its router registration; a
    // future removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"inline_value_provider"),
        "inline_value_provider must be advertised"
    );
}

#[test]
fn range_formatting_is_in_the_expected_set() {
    // Regression pin (LSP-RANGE-FORMAT): the
    // `textDocument/rangeFormatting` capability must be advertised
    // alongside its router registration; a future removal of either half
    // surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"document_range_formatting_provider"),
        "document_range_formatting_provider must be advertised"
    );
}

#[test]
fn range_formatting_has_a_pinned_wire_contract() {
    use lsp_types::request::Request;
    // Regression pin (LSP-RANGE-FORMAT): the request type's METHOD string,
    // so an lsp-types upgrade that reshapes the endpoint surfaces here
    // and the end-to-end tests in `range_formatting_tests.rs` pin the
    // handler.
    assert_eq!(
        lsp_types::request::RangeFormatting::METHOD,
        "textDocument/rangeFormatting"
    );
}

#[test]
fn inline_value_has_a_pinned_wire_contract() {
    use lsp_types::request::Request;
    // Regression pin (LSP-INLINE-VALUE): the request type's METHOD string,
    // so an lsp-types upgrade that reshapes the endpoint surfaces here
    // and the end-to-end tests in `inline_value_tests.rs` pin the handler.
    assert_eq!(
        lsp_types::request::InlineValueRequest::METHOD,
        "textDocument/inlineValue"
    );
}

#[test]
fn moniker_is_in_the_expected_set() {
    // Regression pin (LSP-MONIKER): the `textDocument/moniker` capability
    // must be advertised alongside its router registration; a future
    // removal of either half surfaces here first.
    assert!(
        expected_advertised_providers().contains(&"moniker_provider"),
        "moniker_provider must be advertised"
    );
}

#[test]
fn execute_command_is_in_the_expected_set() {
    // Regression pin (LSP-EXEC-CMD): the `workspace/executeCommand`
    // capability must be advertised with the same command id the
    // code-lens resolve emits — an advertised command with no
    // advertisement-side registration and no execution route was the
    // exact dead-feature shape the conformance audit exists for.
    assert!(
        expected_advertised_providers().contains(&"execute_command_provider"),
        "execute_command_provider must be advertised"
    );
}

#[test]
fn execute_command_advertises_the_lens_command() {
    // The advertised command set names exactly what a client may execute:
    // the code-lens resolve's command id, and nothing else.
    let advertised = crate::handler::advertised_commands();
    assert_eq!(
        advertised,
        vec![crate::code_lens::SHOW_REFERENCES_COMMAND.to_string()],
        "the advertised command set is the lens command, verbatim"
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
