//! Shared test utilities for glyim-def-map tests.

use crate::{CrateDefMap, ModuleId, Resolver, build_def_map};
use glyim_core::def_id::CrateId;
use glyim_diag::GlyimDiagnostic;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;

/// Parse source code and build a def map, returning both the map and any diagnostics.
pub fn parse_and_build(source: &str) -> (CrateDefMap, Vec<GlyimDiagnostic>) {
    let parse_result = parse_to_syntax(source, FileId::BOGUS);
    build_def_map(
        &parse_result.root,
        CrateId::from_raw(0),
        glyim_core::interner::Interner::new(),
    )
}

/// Create a Resolver for the given module in the def map.
pub fn resolver_for<'a>(def_map: &'a CrateDefMap, module: ModuleId) -> Resolver<'a> {
    Resolver::new(&def_map.modules, def_map.root, module)
}

/// Find a child module by name within a parent module.
pub fn find_child_module(def_map: &CrateDefMap, parent: ModuleId, name: &str) -> Option<ModuleId> {
    let interned_name = def_map.interner.intern(name);
    def_map.modules[parent]
        .children
        .iter()
        .find(|(n, _)| *n == interned_name)
        .map(|(_, id)| *id)
}
