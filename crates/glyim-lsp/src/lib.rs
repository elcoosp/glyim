pub mod database;
pub mod symbol_index;
pub mod reference_graph;
pub mod driver;
pub mod diagnostics;
pub mod completion;
pub mod hover;
pub mod navigation;
pub mod formatting;
pub mod code_action;
pub mod folding;
pub mod server;
pub mod handler;

pub use database::AnalysisDatabase;
pub use symbol_index::{SymbolIndex, SymbolInfo, SymbolKind, DefinitionLocation, TypeSignature};
pub use reference_graph::{ReferenceGraph, Reference, ReferenceKind};

#[cfg(test)]
mod tests;
