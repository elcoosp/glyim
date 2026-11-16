//! Language Server Protocol implementation for Glyim compiler.
// Stylistic clippy lints suppressed crate-wide (test-noise lints).

/// `call_hierarchy` (LSP-CALL-HIERARCHY).
pub mod call_hierarchy;
/// `code_action`.
pub mod code_action;
/// `code_lens` (LSP-CODE-LENS).
pub mod code_lens;
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
/// `document_link` (LSP-DOC-LINK).
pub mod document_link;
/// driver.
pub mod driver;
/// `execute_command` (LSP-EXEC-CMD).
pub mod execute_command;
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
/// `inline_value` (LSP-INLINE-VALUE).
pub mod inline_value;
/// `linked_editing` (LSP-LINKED-EDIT).
pub mod linked_editing;
/// `moniker` (LSP-MONIKER).
pub mod moniker;
/// navigation.
pub mod navigation;
/// `on_type_formatting` (LSP-ON-TYPE-FORMAT).
pub mod on_type_formatting;
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
/// `type_hierarchy` (LSP-TYPE-HIERARCHY).
pub mod type_hierarchy;
/// uri.
pub mod uri; // Make database public
/// `workspace_diagnostics` (LSP-WORKSPACE-DIAG).
pub mod workspace_diagnostics;

#[cfg(test)]
mod tests;

pub use call_hierarchy::{
    CallGraphSource, CallSite, IncomingCall, ItemDefinition, OutgoingCall,
    ReferenceGraphCallSource, compute_incoming_calls, compute_outgoing_calls, incoming_calls,
    outgoing_calls, prepare_call_hierarchy, prepare_target,
};
pub use code_lens::{
    GraphLensSource, ItemLens, LensItem, LensSource, SHOW_REFERENCES_COMMAND, code_lenses,
    compute_code_lenses, resolve_code_lens,
};
pub use database::AnalysisDatabase;
pub use database::{FileMap, SourceMap};
pub use document_highlight::{
    HighlightKind, HighlightSpan, OccurrenceSource, ReferenceGraphSource, compute_highlights,
    document_highlights,
};
pub use document_link::{
    IncludeLink, IncludeMacroKind, LinkLexSource, LinkToken, ParseTreeLinkSource, document_links,
    link_targets,
};
pub use execute_command::{
    ExecuteCommandError, GraphReferenceLookup, ReferenceLookup, ShowReferencesArgs,
    compute_reference_targets, execute_command, parse_show_references_args,
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
pub use inline_value::{
    HirInlineValueSource, InlineValueFact, InlineValueSource, RecordedBinding,
    compute_inline_values, inline_values, wire_inline_value,
};
pub use linked_editing::linked_editing_ranges;
pub use moniker::{
    FactKind, MONIKER_SCHEME, MonikerFact, MonikerSource, RecordedSymbol, SessionMonikerSource,
    SpelledImport, monikers, monikers_at, wire_moniker,
};
pub use on_type_formatting::{
    BracketToken, OnTypeIndent, OnTypeTokenSource, ParseTreeOnTypeSource, on_type_formatting,
    on_type_indent,
};
pub use prepare_rename::{prepare_rename, rename_target};
pub use pull_diagnostics::{
    AffectedFilesSource, DependencyGraphSource, DiagnosticSource, RecordedDiagnosticSource,
    pull_diagnostics, pull_report, related_reports,
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
pub use type_hierarchy::{
    HierarchyItemKind, HierarchyMember, HirTypeHierarchySource, NamedItem, TypeHierarchySource,
    compute_subtypes, compute_supertypes, hierarchy_target, prepare_type_hierarchy, type_subtypes,
    type_supertypes,
};
pub use workspace_diagnostics::{
    AnalysedFile, AnalysedSetSource, RecordedAnalysedSource, workspace_diagnostics,
    workspace_report,
};
