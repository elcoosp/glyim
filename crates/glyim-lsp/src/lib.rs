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
/// `reference_graph`.
pub mod reference_graph;
/// rename.
pub mod rename;
/// server.
pub mod server;
/// state.
pub mod state;
/// `symbol_index`.
pub mod symbol_index;
/// uri.
pub mod uri; // Make database public

#[cfg(test)]
mod tests;

pub use database::AnalysisDatabase;
pub use database::{FileMap, SourceMap};
pub use goto_definition::goto_definition;
pub use reference_graph::{Reference, ReferenceGraph, ReferenceKind};
pub use state::LspState;
pub use symbol_index::{DefinitionLocation, SymbolIndex, SymbolInfo, SymbolKind, TypeSignature};
