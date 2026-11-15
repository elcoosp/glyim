//! Language Server Protocol implementation for Glyim compiler.
// Stylistic clippy lints suppressed crate-wide (test-noise lints).

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
/// `goto_definition`.
pub mod goto_definition;
/// handler.
pub mod handler;
/// hover.
pub mod hover;
/// navigation.
pub mod navigation;
/// `prepare_rename` (LSP-PREPARE-RENAME).
pub mod prepare_rename;
/// `reference_graph`.
pub mod reference_graph;
/// rename.
pub mod rename;
/// `selection_range` (LSP-SELECTION).
pub mod selection_range;
/// server.
pub mod server;
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

pub use database::AnalysisDatabase;
pub use database::{FileMap, SourceMap};
pub use document_highlight::{
    HighlightKind, HighlightSpan, OccurrenceSource, ReferenceGraphSource, compute_highlights,
    document_highlights,
};
pub use goto_definition::goto_definition;
pub use prepare_rename::{prepare_rename, rename_target};
pub use reference_graph::{Reference, ReferenceGraph, ReferenceKind};
pub use selection_range::{
    AncestorSpanSource, ParseTreeSource, compute_selection_chains, selection_ranges,
};
pub use state::LspState;
pub use symbol_index::{DefinitionLocation, SymbolIndex, SymbolInfo, SymbolKind, TypeSignature};
pub use type_definition::{
    SymbolIndexSource, TypeSymbolSource, named_type_of, type_definition, type_target,
};
