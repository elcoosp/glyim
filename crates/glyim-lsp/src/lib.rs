//! Language Server Protocol implementation for Glyim compiler.
// Stylistic clippy lints suppressed crate-wide (test-noise lints).

/// `call_hierarchy` (LSP-CALL-HIERARCHY).
pub mod call_hierarchy;
/// `code_action`.
pub mod code_action;
/// completion.
pub mod completion;
/// database.
pub mod database;
/// `dep_graph`.
pub mod dep_graph;
/// diagnostics.
pub mod diagnostics;
/// `document_highlight` (LSP-HIGHLIGHT).
pub mod document_highlight;
/// driver.
pub mod driver;
/// folding.
pub mod folding;
/// formatting.
pub mod formatting;
/// `goto_declaration` (LSP-DECLARATION).
pub mod goto_declaration;
/// `goto_definition`.
pub mod goto_definition;
/// handler.
pub mod handler;
/// hover.
pub mod hover;
/// `implementation` (LSP-IMPLEMENTATION).
pub mod implementation;
/// `inlay_hint` (LSP-INLAY-HINT).
pub mod inlay_hint;
/// navigation.
pub mod navigation;
/// `prepare_rename` (LSP-PREPARE-RENAME).
pub mod prepare_rename;
/// `pull_diagnostics` (LSP-PULL-DIAG).
pub mod pull_diagnostics;
/// `reference_graph`.
pub mod reference_graph;
/// rename.
pub mod rename;
/// `selection_range` (LSP-SELECTION).
pub mod selection_range;
/// `semantic_tokens` (LSP-SEMANTIC-TOKENS).
pub mod semantic_tokens;
/// server.
pub mod server;
/// `server_log` (LSP-LOG-FILE).
pub mod server_log;
/// `signature_help` (LSP-SIGNATURE-HELP).
pub mod signature_help;
/// state.
pub mod state;
/// `symbol_index`.
pub mod symbol_index;
/// `type_definition` (LSP-TYPE-DEF).
pub mod type_definition;
/// uri.
pub mod uri; // Make database public

#[cfg(test)]
mod tests;

pub use call_hierarchy::{
    CallGraphSource, CallSite, IncomingCall, ItemDefinition, OutgoingCall,
    ReferenceGraphCallSource, compute_incoming_calls, compute_outgoing_calls, incoming_calls,
    outgoing_calls, prepare_call_hierarchy, prepare_target,
};
pub use database::AnalysisDatabase;
pub use database::{FileMap, SourceMap};
pub use document_highlight::{
    HighlightKind, HighlightSpan, OccurrenceSource, ReferenceGraphSource, compute_highlights,
    document_highlights,
};
pub use goto_declaration::{
    DeclarationSource, ParseTreeDeclarationSource, declaration_target, goto_declaration,
    import_bindings,
};
pub use goto_definition::goto_definition;
pub use implementation::{
    HirImplSource, ImplSite, ImplSource, goto_implementation, implementation_targets,
};
pub use inlay_hint::{
    BindingTypeSource, HirBindingTypeSource, UnannotatedBinding, compute_inlay_hints, inlay_hints,
    render_type_label,
};
pub use prepare_rename::{prepare_rename, rename_target};
pub use pull_diagnostics::{
    DiagnosticSource, RecordedDiagnosticSource, pull_diagnostics, pull_report,
};
pub use reference_graph::{Reference, ReferenceGraph, ReferenceKind};
pub use selection_range::{
    AncestorSpanSource, ParseTreeSource, compute_selection_chains, selection_ranges,
};
pub use semantic_tokens::{
    ClassifiedToken, LexedToken, ParseTreeTokenSource, TOKEN_TYPES, TokenLexSource,
    classify_tokens, encode_delta, semantic_tokens_full, semantic_tokens_range, token_legend,
    token_type_for,
};
pub use server_log::{FileLog, LogInitError, install_global};
pub use signature_help::{
    CallSiteSource, CalledFunction, ReferenceGraphCallSiteSource, SignatureData, SignatureHelpPlan,
    SignatureIndexSource, SignatureSource, compute_signature_help, signature_help,
};
pub use state::LspState;
pub use symbol_index::{DefinitionLocation, SymbolIndex, SymbolInfo, SymbolKind, TypeSignature};
pub use type_definition::{
    SymbolIndexSource, TypeSymbolSource, named_type_of, type_definition, type_target,
};
