//! Module graph and lightweight definition map.
// Stylistic clippy lints suppressed crate-wide (test-noise lints).
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::vec_init_then_push,
    clippy::assertions_on_constants,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::manual_c_str_literals,
    clippy::doc_lazy_continuation,
    clippy::empty_line_after_doc_comments,
    clippy::manual_strip,
    clippy::needless_range_loop,
    clippy::unnecessary_cast,
    clippy::clone_on_copy,
    clippy::mutable_key_type,
    clippy::only_used_in_recursion,
    clippy::let_unit_value,
    clippy::unnecessary_literal_unwrap,
    clippy::format_in_format_args,
    clippy::permissions_set_readonly_false,
    clippy::needless_lifetimes,
    clippy::collapsible_if
)]

use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, LocalDefId, VariantIdx};
use glyim_core::interner::{Interner, Name};
use glyim_core::path::{Path, PathKind, PathSegment};
use glyim_core::primitives::Visibility;
use glyim_diag::GlyimDiagnostic;
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use glyim_syntax::{SyntaxElement, SyntaxKind, SyntaxNode};
use indexmap::IndexMap;
use std::collections::HashMap;

glyim_core::define_idx!(ModuleId);

#[derive(Clone, Debug)]
/// CrateDefMap.
pub struct CrateDefMap {
/// Struct.
    pub root: ModuleId,
/// Struct.
    pub modules: IndexVec<ModuleId, ModuleData>,
/// Struct.
    pub krate: CrateId,
/// Struct.
    pub interner: Interner,
    /// Reverse map from a variant's value-namespace `LocalDefId` to the
    /// enclosing enum's `LocalDefId` and the variant's index. Populated while
    /// declaring enum items so value paths like `Enum::Variant` / `Variant`
    /// resolve to a concrete `(AdtId, VariantIdx)`.
    pub variant_map: HashMap<LocalDefId, (LocalDefId, VariantIdx)>,
}

#[derive(Clone, Debug)]
/// ModuleData.
pub struct ModuleData {
/// Struct.
    pub parent: Option<ModuleId>,
#[doc = "field"]
    pub children: Vec<(Name, ModuleId)>,
/// Struct.
    pub scope: ItemScope,
/// Struct.
    pub origin: ModuleOrigin,
/// Struct.
    pub span: Span,
    /// Every module has a unique `LocalDefId` so it can be referred to by `use` paths
    pub def_id: LocalDefId,
    /// Visibility of the module itself (`pub mod` or private)
    pub visibility: Visibility,
}

impl ModuleData {
/// resolve.
    pub fn resolve(&self, name: Name) -> Option<(LocalDefId, Visibility)> {
        self.scope.resolve(name)
    }
}

#[derive(Clone, Debug)]
/// ModuleOrigin.
pub enum ModuleOrigin {
/// Variant.
    File {
        /// file_id field.
        file_id: FileId,
    },
/// Variant.
    Inline {
        /// span field.
        span: Span,
    },
/// Variant.
    CrateRoot,
}

#[derive(Clone, Debug, Default)]
/// ItemScope.
pub struct ItemScope {
#[doc = "field"]
    pub types: IndexMap<Name, (LocalDefId, Visibility, Span)>,
#[doc = "field"]
    pub values: IndexMap<Name, (LocalDefId, Visibility, Span)>,
#[doc = "field"]
    pub macros: IndexMap<Name, (LocalDefId, Visibility, Span)>,
}

impl ItemScope {
/// resolve.
    pub fn resolve(&self, name: Name) -> Option<(LocalDefId, Visibility)> {
        if let Some((id, vis, _)) = self.types.get(&name) {
            return Some((*id, vis.clone()));
        }
        if let Some((id, vis, _)) = self.values.get(&name) {
            return Some((*id, vis.clone()));
        }
        None
    }

/// declare.
    pub fn declare(
        &mut self,
        name: Name,
        id: LocalDefId,
        vis: Visibility,
        span: Span,
        ns: Namespace,
    ) {
        let entry = (id, vis, span);
        match ns {
            Namespace::Types => {
                self.types.insert(name, entry);
            }
            Namespace::Values => {
                self.values.insert(name, entry);
            }
            Namespace::Macros => {
                self.macros.insert(name, entry);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Namespace.
pub enum Namespace {
/// Variant.
    Types,
/// Variant.
    Values,
/// Variant.
    Macros,
}

#[derive(Clone, Debug, Default)]
/// PerNs.
pub struct PerNs {
#[doc = "field"]
    pub types: Option<(LocalDefId, Visibility)>,
#[doc = "field"]
    pub values: Option<(LocalDefId, Visibility)>,
#[doc = "field"]
    pub macros: Option<(LocalDefId, Visibility)>,
}

impl PerNs {
/// is_none.
    pub fn is_none(&self) -> bool {
        self.types.is_none() && self.values.is_none() && self.macros.is_none()
    }
/// from_types.
    pub fn from_types(id: LocalDefId, vis: Visibility) -> Self {
        Self {
            types: Some((id, vis)),
            values: None,
            macros: None,
        }
    }
}

/// Resolver.
pub struct Resolver<'a> {
    modules: &'a IndexVec<ModuleId, ModuleData>,
    root: ModuleId,
    module: ModuleId,
}

impl<'a> Resolver<'a> {
/// new.
    pub fn new(
        modules: &'a IndexVec<ModuleId, ModuleData>,
        root: ModuleId,
        module: ModuleId,
    ) -> Self {
        Self {
            modules,
            root,
            module,
        }
    }

/// resolve_path.
    pub fn resolve_path(&self, path: &Path) -> PerNs {
        let mut current_module = self.module;
        let start_idx = match path.kind {
            PathKind::Plain => 0,
            PathKind::SelfPath => 0,
            PathKind::Super(n) => {
                let mut module = current_module;
                for _ in 0..n {
                    if let Some(parent) = self.modules[module].parent {
                        module = parent;
                    } else {
                        break;
                    }
                }
                current_module = module;
                0
            }
            PathKind::Crate => {
                current_module = self.root;
                0
            }
        };

        for (i, segment) in path.segments.iter().enumerate().skip(start_idx) {
            let module_data = &self.modules[current_module];
            if i == path.segments.len() - 1 {
                // Final segment: search the current module *and its ancestors*
                // (walking up the `parent` chain) so that a bare name referenced
                // inside a nested `mod` block resolves to an item declared in
                // that same module without requiring a crate-wide `pub use`
                // re-export (Rust's lexical scoping for paths). Only the final
                // segment is subject to this upward search; intermediate
                // segments are pure module navigation.
                let mut search_mod = current_module;
                loop {
                    let scope = &self.modules[search_mod].scope;
                    let mut result = PerNs::default();
                    if let Some((tid, tvis, _)) = scope.types.get(&segment.name) {
                        result.types = Some((*tid, tvis.clone()));
                    }
                    if let Some((vid, vvis, _)) = scope.values.get(&segment.name) {
                        result.values = Some((*vid, vvis.clone()));
                    }
                    if result.types.is_some() || result.values.is_some() {
                        return result;
                    }
                    match self.modules[search_mod].parent {
                        Some(parent) => search_mod = parent,
                        None => break,
                    }
                }
                return PerNs::default();
            } else if let Some((_, child_id)) = module_data
                .children
                .iter()
                .find(|(n, _)| *n == segment.name)
            {
                current_module = *child_id;
            } else {
                return PerNs::default();
            }
        }
        PerNs::default()
    }

/// def_map.
    pub fn def_map(&self) -> &IndexVec<ModuleId, ModuleData> {
        self.modules
    }

/// Module.
    pub fn module(&self) -> ModuleId {
        self.module
    }
}

/// Helper to resolve a path that ends at a module, returning the ModuleId.
fn resolve_module_path_for_modules(
    modules: &IndexVec<ModuleId, ModuleData>,
    start_module: ModuleId,
    path: &Path,
) -> Option<ModuleId> {
    let mut current_module = start_module;
    let start_idx = match path.kind {
        PathKind::Plain => 0,
        PathKind::SelfPath => 0,
        PathKind::Super(n) => {
            let mut module = current_module;
            for _ in 0..n {
                if let Some(parent) = modules[module].parent {
                    module = parent;
                } else {
                    return None;
                }
            }
            current_module = module;
            0
        }
        PathKind::Crate => {
            current_module = modules[start_module].parent.unwrap_or(start_module);
            while let Some(parent) = modules[current_module].parent {
                current_module = parent;
            }
            0
        }
    };

    for segment in path.segments.iter().skip(start_idx) {
        let module_data = &modules[current_module];
        if let Some((_, child_id)) = module_data
            .children
            .iter()
            .find(|(n, _)| *n == segment.name)
        {
            current_module = *child_id;
        } else {
            return None;
        }
    }
    Some(current_module)
}

fn import_all_public_for_modules(
    source: ModuleId,
    target: ModuleId,
    modules: &mut IndexVec<ModuleId, ModuleData>,
) {
    let source_scope = modules[source].scope.clone();

    for (name, (id, vis, span)) in source_scope.types {
        if vis == Visibility::Public {
            modules[target]
                .scope
                .declare(name, id, vis, span, Namespace::Types);
        }
    }
    for (name, (id, vis, span)) in source_scope.values {
        if vis == Visibility::Public {
            modules[target]
                .scope
                .declare(name, id, vis, span, Namespace::Values);
        }
    }
}

/// Extracts an alias from a `use foo as bar;` declaration.
/// Returns `Some(Name)` for `as bar`, `Some(_)` for `as _`, and `None` if no alias is present.
fn extract_alias_from_use_tree(node: &SyntaxNode, interner: &Interner) -> Option<Name> {
    let mut found_as = false;
    for elem in node.children_with_tokens() {
        if let Some(token) = elem.as_token() {
            if token.kind() == SyntaxKind::KwAs {
                found_as = true;
            } else if found_as && token.kind() == SyntaxKind::Ident {
                return Some(interner.intern(token.text()));
            }
        }
    }
    None
}

/// Extract a `Path` from a syntax node (UseTree or PathExpr).
fn extract_path_from_syntax(node: &SyntaxNode, interner: &Interner) -> Option<Path> {
    let mut segments: Vec<PathSegment> = Vec::new();
    let mut kind = PathKind::Plain;
    let mut super_count = 0u32;

    fn visit(
        n: &SyntaxNode,
        segments: &mut Vec<PathSegment>,
        kind: &mut PathKind,
        super_count: &mut u32,
        interner: &Interner,
    ) {
        if n.kind() == SyntaxKind::PathExpr {
            for child in n.children() {
                visit(&child, segments, kind, super_count, interner);
            }
            return;
        }

        for elem in n.children_with_tokens() {
            if let Some(token) = elem.as_token() {
                match token.kind() {
                    SyntaxKind::KwCrate => *kind = PathKind::Crate,
                    SyntaxKind::KwSelf => *kind = PathKind::SelfPath,
                    SyntaxKind::KwSuper => {
                        *super_count += 1;
                        *kind = PathKind::Super(*super_count);
                    }
                    SyntaxKind::Ident => {
                        let name = interner.intern(token.text());
                        segments.push(PathSegment { name, generic_args: None });
                    }
                    // Non-path tokens (punctuation, whitespace, etc.) are silently
                    // skipped — only identifiers and keywords contribute to the path.
                    _ => {}
                }
            } else if let Some(child_node) = elem.as_node() {
                visit(child_node, segments, kind, super_count, interner);
            }
        }
    }

    visit(node, &mut segments, &mut kind, &mut super_count, interner);

    if segments.is_empty() && super_count > 0 {
        return Some(Path {
            segments,
            kind: PathKind::Super(super_count),
        });
    }
    if segments.is_empty() && kind == PathKind::Crate {
        return Some(Path {
            segments,
            kind: PathKind::Crate,
        });
    }

    if !segments.is_empty() {
        Some(Path { segments, kind })
    } else {
        None
    }
}

#[tracing::instrument(skip(root))]
/// build_def_map.
pub fn build_def_map(
    root: &SyntaxNode,
    krate: CrateId,
    mut interner: Interner,
) -> (CrateDefMap, Vec<GlyimDiagnostic>) {
    let mut diagnostics = Vec::new();
    let mut modules: IndexVec<ModuleId, ModuleData> = IndexVec::new();
    let mut def_counter: u32 = 1;
    let mut def_to_module: HashMap<LocalDefId, ModuleId> = HashMap::new();
    // Plan §4.1: collect `use` declarations during the structural pass instead
    // of resolving them inline, so import resolution can run as a fixed-point
    // loop afterward (required for cross-module globs and import cycles).
    let mut use_decls: Vec<(SyntaxNode, ModuleId, Visibility)> = Vec::new();
    let mut variant_map: HashMap<LocalDefId, (LocalDefId, VariantIdx)> = HashMap::new();

    let root_module = modules.push(ModuleData {
        parent: None,
        children: Vec::new(),
        scope: ItemScope::default(),
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(0), // root gets id 0
        visibility: Visibility::Public,  // root is always public
    });
    def_to_module.insert(LocalDefId::from_raw(0), root_module);

    collect_items(
        root,
        root_module,
        &mut modules,
        &mut diagnostics,
        &interner,
        &mut def_counter,
        &mut def_to_module,
        &mut use_decls,
        &mut variant_map,
    );

    // FFI import declarations: `extern "C" { fn name(...); }` blocks may appear
    // at module top level OR nested inside function bodies. Their inner `fn`s
    // are foreign (C-linkage) symbols that must resolve as callables from
    // anywhere in the crate. The structural `collect_items` pass only walks
    // top-level module children, so nested-in-body blocks never reach it. We
    // therefore scan the *entire* syntax tree for `ExternBlock` nodes and
    // register each inner `fn` in the crate-root value namespace (FFI has
    // global linkage). The downstream type-checker resolves each lowered fn
    // item by `item.name` through the def-map (see `typeck_crate`), so this
    // registration is what makes `extern "C" { fn foo(); }` calls resolve.
    collect_extern_imports(
        root,
        root_module,
        &mut modules,
        &mut diagnostics,
        &mut interner,
        &mut def_counter,
        &mut def_to_module,
    );

    // Plan §4.1: fixed-point import resolution. Re-run every pending `use`
    // declaration until a full pass makes no progress (resolves no new name).
    // `declare` is idempotent (IndexMap insert), so re-processing settled
    // imports is a no-op. Cross-module globs / cycles can only be resolved once
    // all modules and their public items are present, which the structural pass
    // guarantees before this loop runs.
    let mut prev_count = scope_entry_count(&modules);
    loop {
        for (node, module, vis) in &use_decls {
            process_use_decl(node, *module, &mut modules, &interner, vis.clone());
        }
        let new_count = scope_entry_count(&modules);
        if new_count == prev_count {
            break;
        }
        prev_count = new_count;
        // Guard against non-terminating resolution in pathological inputs.
        if prev_count > modules.len() * 1024 {
            diagnostics.push(GlyimDiagnostic::parse_error(
                Span::DUMMY,
                "import resolution did not reach a fixed point".to_string(),
            ));
            break;
        }
    }

    validate_import_visibility(&modules, &def_to_module, &interner, &mut diagnostics);

    let def_map = CrateDefMap {
        root: root_module,
        modules,
        krate,
        interner,
        variant_map,
    };
    (def_map, diagnostics)
}

/// Total number of declarations across all module scopes (used by the §4.1
/// fixed-point import-resolution loop to detect when a pass makes no progress).
fn scope_entry_count(modules: &IndexVec<ModuleId, ModuleData>) -> usize {
    modules
        .iter()
        .map(|m| m.scope.types.len() + m.scope.values.len() + m.scope.macros.len())
        .sum()
}

fn process_use_decl(
    node: &SyntaxNode,
    parent_module: ModuleId,
    modules: &mut IndexVec<ModuleId, ModuleData>,
    interner: &Interner,
    use_vis: Visibility,
) {
    let use_tree = match node.children().find(|n| n.kind() == SyntaxKind::UseTree) {
        Some(t) => t,
        None => return,
    };
    process_use_tree(&use_tree, parent_module, modules, interner, use_vis);
}

fn process_use_tree(
    node: &SyntaxNode,
    parent_module: ModuleId,
    modules: &mut IndexVec<ModuleId, ModuleData>,
    interner: &Interner,
    use_vis: Visibility,
) {
    // Check all children (nodes and tokens) for special markers
    let has_glob = node
        .children_with_tokens()
        .any(|e| e.kind() == SyntaxKind::Star);
    let has_nested = node
        .children_with_tokens()
        .any(|e| e.kind() == SyntaxKind::LBrace);
    let use_path_node = node.children().find(|n| n.kind() == SyntaxKind::UsePath);

    // Handle glob import: use std::io::*;
    if has_glob {
        if let Some(path_node) = use_path_node {
            let path = extract_path_from_syntax(&path_node, interner);
            if let Some(p) = path
                && let Some(mod_id) = resolve_module_path_for_modules(modules, parent_module, &p)
            {
                import_all_public_for_modules(mod_id, parent_module, modules);
            }
        }
        return;
    }

    // Handle nested import: use std::io::{Read, Write};
    if has_nested {
        if let Some(path_node) = use_path_node {
            let path = extract_path_from_syntax(&path_node, interner);

            // Get the base module for the path before the braces
            let base_module = if let Some(p) = path {
                resolve_module_path_for_modules(modules, parent_module, &p)
            } else {
                None
            };

            // Process each item inside the braces
            for child in node.children() {
                if child.kind() == SyntaxKind::UseTree {
                    let inner_use_path = child.children().find(|n| n.kind() == SyntaxKind::UsePath);
                    if let Some(inner_path_node) = inner_use_path {
                        let inner_path = extract_path_from_syntax(&inner_path_node, interner);

                        if let (Some(base_mod), Some(inner_p)) = (base_module, inner_path)
                            && inner_p.segments.len() == 1
                        {
                            let orig_name = inner_p.segments[0].name;
                            let inner_alias = extract_alias_from_use_tree(&child, interner);
                            let bind_name = inner_alias.unwrap_or(orig_name);

                            // Handle submodule import (e.g. `use std::io::{self, Read}` => `io` itself)
                            if let Some(child_mod_id) = modules[base_mod]
                                .children
                                .iter()
                                .find(|(n, _)| *n == orig_name)
                                .map(|(_, id)| *id)
                            {
                                let module_data = &modules[child_mod_id];
                                let def_id = module_data.def_id;
                                modules[parent_module].scope.declare(
                                    bind_name,
                                    def_id,
                                    use_vis.clone(),
                                    node_span(&child),
                                    Namespace::Types,
                                );
                            }

                            // Now resolve types/values for the inner segment
                            let resolver = Resolver::new(modules, ModuleId::from_raw(0), base_mod);
                            let per_ns = resolver.resolve_path(&inner_p);

                            if let Some((id, _)) = per_ns.types {
                                modules[parent_module].scope.declare(
                                    bind_name,
                                    id,
                                    use_vis.clone(),
                                    node_span(&child),
                                    Namespace::Types,
                                );
                            }
                            if let Some((id, _)) = per_ns.values {
                                modules[parent_module].scope.declare(
                                    bind_name,
                                    id,
                                    use_vis.clone(),
                                    node_span(&child),
                                    Namespace::Values,
                                );
                            }
                        }
                    }
                }
            }
        }
        return;
    }

    // Simple path: use foo::bar;
    if let Some(path_node) = use_path_node {
        let path = extract_path_from_syntax(&path_node, interner);
        if let Some(path) = path {
            let alias = extract_alias_from_use_tree(node, interner);

            // NEW: Handle module import (e.g. `use std::io;` where `io` is a module)
            if let Some(mod_id) = resolve_module_path_for_modules(modules, parent_module, &path) {
                let module_data = &modules[mod_id];
                let def_id = module_data.def_id;
                let name = alias.or_else(|| path.segments.last().map(|s| s.name));
                if let Some(name) = name {
                    modules[parent_module].scope.declare(
                        name,
                        def_id,
                        use_vis.clone(),
                        node_span(node),
                        Namespace::Types,
                    );
                }
            }

            // Existing type/value resolution (unchanged)
            let resolver = Resolver::new(modules, ModuleId::from_raw(0), parent_module);
            let per_ns = resolver.resolve_path(&path);

            let name = alias.or_else(|| path.segments.last().map(|s| s.name));

            if let Some(name) = name {
                if let Some((id, _)) = per_ns.types {
                    modules[parent_module].scope.declare(
                        name,
                        id,
                        use_vis.clone(),
                        node_span(node),
                        Namespace::Types,
                    );
                }
                if let Some((id, _)) = per_ns.values {
                    modules[parent_module].scope.declare(
                        name,
                        id,
                        use_vis.clone(),
                        node_span(node),
                        Namespace::Values,
                    );
                }
                if let Some((id, _)) = per_ns.macros {
                    modules[parent_module].scope.declare(
                        name,
                        id,
                        use_vis.clone(),
                        node_span(node),
                        Namespace::Macros,
                    );
                }
            }
        }
    }
}

/// Collect items from a syntax node (SourceFile or Module node) into the given module.
/// For a Module node, the node itself is the container; its children (nodes only) are the items.
fn collect_items(
    node: &SyntaxNode,
    parent_module: ModuleId,
    modules: &mut IndexVec<ModuleId, ModuleData>,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    interner: &Interner,
    def_counter: &mut u32,
    def_to_module: &mut HashMap<LocalDefId, ModuleId>,
    use_decls: &mut Vec<(SyntaxNode, ModuleId, Visibility)>,
    variant_map: &mut HashMap<LocalDefId, (LocalDefId, VariantIdx)>,
) {
    for child in node.children() {
        match child.kind() {
            // Inline module: `mod name { ... }`
            SyntaxKind::Module => {
                let name_str = extract_module_name(&child);
                let name = interner.intern(&name_str);
                let span = node_span(&child);
                let vis = visibility_of_node(&child, interner);
                let is_dup = modules[parent_module]
                    .children
                    .iter()
                    .any(|(n, _)| *n == name);
                if is_dup {
                    diagnostics.push(GlyimDiagnostic::parse_error(
                        span,
                        format!("duplicate module `{}`", interner.resolve(name)),
                    ));
                } else {
                    let child_module = modules.push(ModuleData {
                        parent: Some(parent_module),
                        children: Vec::new(),
                        scope: ItemScope::default(),
                        origin: ModuleOrigin::Inline { span },
                        span,
                        def_id: LocalDefId::from_raw(*def_counter),
                        visibility: vis.clone(),
                    });
                    *def_counter += 1;
                    modules[parent_module].children.push((name, child_module));

                    // Modules are resolvable in the type namespace of their parent.
                    let child_def_id = modules[child_module].def_id;
                    modules[parent_module].scope.declare(
                        name,
                        child_def_id,
                        vis,
                        span,
                        Namespace::Types,
                    );
                    def_to_module.insert(child_def_id, parent_module);

                    // Recurse into the module node itself. Its children (nodes) are the items inside.
                    collect_items(
                        &child,
                        child_module,
                        modules,
                        diagnostics,
                        interner,
                        def_counter,
                        def_to_module,
                        use_decls,
                        variant_map,
                    );
                }
            }

            // Enum: declare the enum type in the *type* namespace (so `Color`
            // resolves as an ADT). Additionally create a synthetic module
            // named after the enum that holds its variants in the *value*
            // namespace, so `Color::Red` resolves `Color` as a module-like
            // scope and descends into it to find `Red`. Record a reverse map
            // from each variant's value `LocalDefId` to
            // `(enum_local, VariantIdx)` for `check_path`.
            SyntaxKind::EnumDef => {
                let name_str = extract_ident(&child);
                let name = interner.intern(&name_str);
                let vis = visibility_of_node(&child, interner);
                let span = node_span(&child);
                let enum_local = LocalDefId::from_raw(*def_counter);
                *def_counter += 1;
                modules[parent_module].scope.declare(
                    name,
                    enum_local,
                    vis.clone(),
                    span,
                    Namespace::Types,
                );
                // The enum *type* is defined in `parent_module` (so an
                // `Inherited` enum is accessible from its own module).
                def_to_module.insert(enum_local, parent_module);

                // Synthetic module for the enum's variants, holding them in
                // the value namespace so `Color::Red` resolves `Color` as a
                // module-like scope. Uses its own fresh def id (distinct from
                // `enum_local`, which identifies the type / ADT).
                let enum_mod_local = LocalDefId::from_raw(*def_counter);
                *def_counter += 1;
                let enum_module = modules.push(ModuleData {
                    parent: Some(parent_module),
                    children: Vec::new(),
                    scope: ItemScope::default(),
                    origin: ModuleOrigin::Inline { span },
                    span,
                    def_id: enum_mod_local,
                    visibility: vis.clone(),
                });
                modules[parent_module].children.push((name, enum_module));
                def_to_module.insert(enum_mod_local, enum_module);

                // Variants live inside a `VariantList` child node, not
                // directly under `EnumDef`.
                let mut variant_idx: u32 = 0;
                for vlist in child.children() {
                    if vlist.kind() != SyntaxKind::VariantList {
                        continue;
                    }
                    for vchild in vlist.children() {
                        if vchild.kind() != SyntaxKind::EnumVariant {
                            continue;
                        }
                        let vname_str = extract_ident(&vchild);
                        let vname = interner.intern(&vname_str);
                        let vvis = visibility_of_node(&vchild, interner);
                        let vspan = node_span(&vchild);
                        let vlocal = LocalDefId::from_raw(*def_counter);
                        *def_counter += 1;
                        modules[enum_module].scope.declare(
                            vname,
                            vlocal,
                            vvis,
                            vspan,
                            Namespace::Values,
                        );
                        variant_map
                            .insert(vlocal, (enum_local, VariantIdx::from_raw(variant_idx)));
                        variant_idx += 1;
                    }
                }
            }

            // Items that go into the namespace
            SyntaxKind::FnDef
            | SyntaxKind::StructDef
            | SyntaxKind::TraitDef
            | SyntaxKind::ImplDef
            | SyntaxKind::TypeAlias
            | SyntaxKind::ConstDef
            | SyntaxKind::StaticDef
            | SyntaxKind::ExternBlock => {
                if let Some(ns) = namespace_for_kind(child.kind()) {
                    let name_str = extract_ident(&child);
                    let name = interner.intern(&name_str);
                    let vis = visibility_of_node(&child, interner);
                    let id = LocalDefId::from_raw(*def_counter);
                    *def_counter += 1;
                    let span = node_span(&child);
                    def_to_module.insert(id, parent_module);

                    let scope = &mut modules[parent_module].scope;
                    let existing = match ns {
                        Namespace::Types => scope.types.contains_key(&name),
                        Namespace::Values => scope.values.contains_key(&name),
                        Namespace::Macros => scope.macros.contains_key(&name),
                    };
                    if existing {
                        diagnostics.push(GlyimDiagnostic::parse_error(
                            span,
                            format!("duplicate definition of `{}`", interner.resolve(name)),
                        ));
                    } else {
                        scope.declare(name, id, vis, span, ns);
                    }
                }
            }

            SyntaxKind::UseDecl => {
                // Plan §4.1: defer resolution to the fixed-point loop in
                // `build_def_map` instead of resolving inline here. The syntax
                // node is an Rc-backed handle and is safe to clone for later
                // re-processing.
                let vis = visibility_of_node(&child, interner);
                use_decls.push((child.clone(), parent_module, vis));
            }

            // Other syntax kinds (comments, expressions inside blocks, etc.) are
            // not item declarations and do not contribute to the def map.
            _ => {}
        }
    }
}

/// Extract the name of an inline module from its `Module` node as a String.
fn extract_module_name(module_node: &SyntaxNode) -> String {
    for child in module_node.children_with_tokens() {
        if let Some(token) = child.as_token()
            && token.kind() == SyntaxKind::Ident
        {
            return token.text().to_string();
        }
    }
    "__unnamed_module".to_string()
}

/// Extract the name of an item (e.g., function, struct) from its syntax node.
/// For `ImplDef`, we generate a synthetic unique name because impls have no inherent name.
/// For `ExternBlock` (used for `extern crate`), handle `as` aliasing.
fn extract_ident(node: &SyntaxNode) -> String {
    if node.kind() == SyntaxKind::ImplDef {
        let offset = u32::from(node.text_range().start());
        return format!("__impl_{}", offset);
    }
    if node.kind() == SyntaxKind::ExternBlock {
        let mut found_as = false;
        let mut last_ident = None;
        for child in node.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == SyntaxKind::KwAs {
                    found_as = true;
                } else if token.kind() == SyntaxKind::Ident {
                    last_ident = Some(token.text().to_string());
                    if found_as {
                        return last_ident.unwrap();
                    }
                }
            }
        }
        if let Some(name) = last_ident {
            return name;
        }
    }
    for child in node.children_with_tokens() {
        if let Some(token) = child.as_token()
            && token.kind() == SyntaxKind::Ident
        {
            return token.text().to_string();
        }
    }
    let offset = u32::from(node.text_range().start());
    format!("__{:?}_anonymous_{}", node.kind(), offset)
}

/// Determine the namespace for a given syntax kind.
fn namespace_for_kind(kind: SyntaxKind) -> Option<Namespace> {
    match kind {
        SyntaxKind::FnDef | SyntaxKind::ConstDef | SyntaxKind::StaticDef => Some(Namespace::Values),
        SyntaxKind::StructDef
        | SyntaxKind::EnumDef
        | SyntaxKind::TraitDef
        | SyntaxKind::ImplDef
        | SyntaxKind::TypeAlias
        | SyntaxKind::ExternBlock => Some(Namespace::Types),
        _ => None,
    }
}

/// Walk the *entire* syntax tree for `extern "C" { fn name(...); }` import
/// blocks and register each inner `fn` in the crate-root value namespace as a
/// `FnDef`. FFI imports have C-linkage (global), so they are callable from any
/// module. Registration happens regardless of where the block appears (module
/// top level or nested inside a function body), which the structural
/// `collect_items` pass cannot do because it only walks top-level module
/// children. The HIR lowering already lowers these inner `fn`s into `FnDef`
/// items (see `lower/mod.rs`'s `ExternBlock` arm), and the type-checker
/// resolves each such item by `item.name` through the def-map — so this
/// declaration is what makes `extern "C" { fn foo(); }` calls resolve to a
/// callable with a registered signature.
fn collect_extern_imports(
    node: &SyntaxNode,
    root_module: ModuleId,
    modules: &mut IndexVec<ModuleId, ModuleData>,
    diagnostics: &mut Vec<GlyimDiagnostic>,
    interner: &mut Interner,
    def_counter: &mut u32,
    def_to_module: &mut HashMap<LocalDefId, ModuleId>,
) {
    for child in node.children() {
        if child.kind() == SyntaxKind::ExternBlock {
            for inner in child.children() {
                if inner.kind() == SyntaxKind::FnDef {
                    let name_str = extract_ident(&inner);
                    let name = interner.intern(&name_str);
                    let id = LocalDefId::from_raw(*def_counter);
                    *def_counter += 1;
                    let span = node_span(&inner);
                    def_to_module.insert(id, root_module);
                    let scope = &mut modules[root_module].scope;
                    // FFI imports have C-linkage (global): the same symbol may be
                    // bound by `extern "C"` blocks in several modules (e.g.
                    // `glyim_thread_unpark` is used by both the `future` and
                    // `thread` stdlib modules). That is valid, so re-declarations
                    // never collide — simply keep the first binding and skip the
                    // rest rather than emitting a hard "duplicate definition"
                    // error (which would abort def-map construction and hide every
                    // downstream type error).
                    if !scope.values.contains_key(&name) {
                        scope.declare(
                            name,
                            id,
                            Visibility::Public,
                            span,
                            Namespace::Values,
                        );
                    }
                }
            }
        }
        // Recurse into every child so nested-in-body `extern` blocks are found.
        collect_extern_imports(
            &child,
            root_module,
            modules,
            diagnostics,
            interner,
            def_counter,
            def_to_module,
        );
    }
}

/// Extract visibility by looking for a `Visibility` node among the node's preceding siblings.
/// Supports `pub`, `pub(crate)`, `pub(super)`, `pub(self)`, and `pub(in path)`.
fn visibility_of_node(node: &SyntaxNode, interner: &Interner) -> Visibility {
    let mut prev = node.prev_sibling_or_token();
    while let Some(sibling) = prev {
        match sibling {
            SyntaxElement::Token(token) => {
                if token.kind() == SyntaxKind::KwPub {
                    return Visibility::Public;
                }
                if token.kind().is_trivia()
                    || token.kind() == SyntaxKind::Comma
                    || token.kind() == SyntaxKind::Semicolon
                {
                    prev = token.prev_sibling_or_token();
                    continue;
                }
                break;
            }
            SyntaxElement::Node(n) => {
                if n.kind() == SyntaxKind::Visibility {
                    let is_pub = n
                        .children_with_tokens()
                        .any(|e| e.kind() == SyntaxKind::KwPub);

                    if is_pub {
                        let has_paren = n
                            .children_with_tokens()
                            .any(|e| e.kind() == SyntaxKind::LParen);

                        if !has_paren {
                            return Visibility::Public;
                        }

                        let has_crate = n
                            .children_with_tokens()
                            .any(|e| e.kind() == SyntaxKind::KwCrate);
                        let has_super = n
                            .children_with_tokens()
                            .any(|e| e.kind() == SyntaxKind::KwSuper);
                        let has_self = n
                            .children_with_tokens()
                            .any(|e| e.kind() == SyntaxKind::KwSelf);

                        if has_crate {
                            return Visibility::PubCrate;
                        } else if has_super {
                            return Visibility::PubSuper;
                        } else if has_self {
                            return Visibility::Inherited;
                        } else {
                            // pub(in path)
                            let mut path_segments = Vec::new();
                            for child in n.children() {
                                if child.kind() == SyntaxKind::UsePath {
                                    for p_child in child.children_with_tokens() {
                                        if let Some(tok) = p_child.as_token()
                                            && tok.kind() == SyntaxKind::Ident
                                        {
                                            path_segments.push(interner.intern(tok.text()));
                                        }
                                    }
                                }
                            }
                            if !path_segments.is_empty() {
                                return Visibility::PubIn(path_segments);
                            }
                        }
                    }
                }
                break;
            }
        }
    }
    Visibility::Inherited
}
/// Create a `Span` from a syntax node's text range.
fn node_span(node: &SyntaxNode) -> Span {
    let range = node.text_range();
    let lo = ByteIdx::from_raw(u32::from(range.start()));
    let hi = ByteIdx::from_raw(u32::from(range.end()));
    Span::new(FileId::BOGUS, lo, hi, SyntaxContext::ROOT)
}

/// Check whether an item with the given visibility, defined in `defining_module`,
/// is accessible from `from_module` within the given `CrateDefMap`.
///
/// # Visibility rules
///
/// - `Visibility::Public`: always accessible from any module.
/// - `Visibility::Inherited`: accessible from the defining module and all of its
///   descendant modules (children, grandchildren, etc.).
/// - `Visibility::Module(id)`: accessible from the module with `ModuleId == id`
///   and all of its descendants.
///
/// # Preconditions
///
/// - `defining_module` must be a valid index into `def_map.modules`.
/// - `from_module` must be a valid index into `def_map.modules`.
pub(crate) fn is_accessible_from(
    vis: Visibility,
    defining_module: ModuleId,
    from_module: ModuleId,
    modules: &IndexVec<ModuleId, ModuleData>,
) -> bool {
    match vis {
        Visibility::Public => true,
        Visibility::PubCrate => {
            // Accessible from anywhere in the same crate.
            // Since def-map is per-crate, this is always true.
            true
        }
        Visibility::PubSuper => {
            // Accessible from the parent module of the defining module and its descendants.
            if let Some(parent) = modules[defining_module].parent {
                from_module == parent || is_descendant_of(from_module, parent, modules)
            } else {
                // If defining_module is the root, pub(super) is equivalent to pub(crate) or private.
                true
            }
        }
        Visibility::PubIn(path) => {
            // Resolve the path relative to the defining module's parent (as per Rust rules)
            let start_module = modules[defining_module].parent.unwrap_or(defining_module);
            if let Some(target_mod) = resolve_module_path_for_modules(
                modules,
                start_module,
                &glyim_core::path::Path {
                    segments: path
                        .iter()
                        .map(|n| glyim_core::path::PathSegment { name: *n, generic_args: None })
                        .collect(),
                    kind: glyim_core::path::PathKind::Plain,
                },
            ) {
                from_module == target_mod || is_descendant_of(from_module, target_mod, modules)
            } else {
                false
            }
        }
        Visibility::Inherited => is_descendant_of(from_module, defining_module, modules),
    }
}

/// Returns `true` if `module` is a descendant of (or equal to) `ancestor`.
///
/// Walks the parent chain from `module` upward; if `ancestor` is encountered,
/// the module is a descendant.
fn is_descendant_of(
    module: ModuleId,
    ancestor: ModuleId,
    modules: &IndexVec<ModuleId, ModuleData>,
) -> bool {
    let mut current = module;
    loop {
        if current == ancestor {
            return true;
        }
        match modules[current].parent {
            Some(parent) => current = parent,
            None => return false,
        }
    }
}

/// Validate that items in each module's scope are accessible from that module.
/// Items imported via `use` that are not accessible will generate a diagnostic.
/// Directly declared items always pass because the defining module equals the
/// current module, and `is_accessible_from` treats same-module access as valid.
fn validate_import_visibility(
    modules: &IndexVec<ModuleId, ModuleData>,
    def_to_module: &HashMap<LocalDefId, ModuleId>,
    interner: &Interner,
    diagnostics: &mut Vec<GlyimDiagnostic>,
) {
    for module_idx in 0..modules.len() {
        let module_id = ModuleId::from_raw(module_idx as u32);
        let scope = &modules[module_id].scope;

        // Check type namespace items
        for (name, (def_id, vis, span)) in &scope.types {
            if let Some(&defining_mod) = def_to_module.get(def_id)
                && !is_accessible_from(vis.clone(), defining_mod, module_id, modules)
            {
                diagnostics.push(GlyimDiagnostic::parse_error(
                    *span,
                    format!(
                        "`{}` is private and not accessible from this module",
                        interner.resolve(*name)
                    ),
                ));
            }
        }

        // Check value namespace items
        for (name, (def_id, vis, span)) in &scope.values {
            if let Some(&defining_mod) = def_to_module.get(def_id)
                && !is_accessible_from(vis.clone(), defining_mod, module_id, modules)
            {
                diagnostics.push(GlyimDiagnostic::parse_error(
                    *span,
                    format!(
                        "`{}` is private and not accessible from this module",
                        interner.resolve(*name)
                    ),
                ));
            }
        }

        // Check macro namespace items
        for (name, (def_id, vis, span)) in &scope.macros {
            if let Some(&defining_mod) = def_to_module.get(def_id)
                && !is_accessible_from(vis.clone(), defining_mod, module_id, modules)
            {
                diagnostics.push(GlyimDiagnostic::parse_error(
                    *span,
                    format!(
                        "macro `{}` is private and not accessible from this module",
                        interner.resolve(*name)
                    ),
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Plan §22.6: `insert_use` — the missing def-map mutation API for auto-import.
//
// Given a source file and an import path (e.g. `std::collections::HashMap`),
// return edited source that adds the `use` statement. Existing top-level `use`
// lines are reused: the new path is appended to the contiguous `use` block at
// the top of the file (deduplicated), otherwise a new `use <path>;` line is
// inserted at the top (after any leading shebang / license comments).
// ---------------------------------------------------------------------------

/// Parse the import path out of a top-level `use` statement line.
/// Handles `use a::b;`, `pub use a::b;`, and `use a::{b, c};` (returns the
/// group head `a::{b, c}` so dedup against the exact same group works).
fn use_line_path(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix("pub use ").or_else(|| trimmed.strip_prefix("use "))?;
    let without_semi = rest.strip_suffix(';').unwrap_or(rest).trim();
    if without_semi.is_empty() {
        None
    } else {
        Some(without_semi.to_string())
    }
}

/// Plan §22.6 auto-import: compute where to insert a `use <import_path>;`
/// statement into `source` and what to insert, returning the byte offset at which
/// to place the insertion and the exact text (newline-terminated) to insert.
/// Reuses the existing top-level `use` block when present. Returns `None` when
/// the path is already imported (idempotent).
pub fn insert_use_edit(source: &str, import_path: &str) -> Option<(usize, String)> {
    let lines: Vec<&str> = source.lines().collect();

    // Collect indices of top-level (non-indented) `use`/`pub use` lines.
    let mut use_lines: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if (trimmed.starts_with("use ") || trimmed.starts_with("pub use "))
            && !line.starts_with(' ')
            && !line.starts_with('\t')
        {
            use_lines.push(i);
        }
    }

    // Idempotency: already imported (exact path, ignoring `pub`)?
    for &i in &use_lines {
        if let Some(p) = use_line_path(lines[i].trim_start()) {
            if p == import_path {
                return None;
            }
        }
    }

    // The text to insert: a newline-terminated `use` line.
    let new_text = format!("use {};\n", import_path);

    if use_lines.is_empty() {
        // No existing use block: insert at the top, after any leading shebang.
        let mut insert_line = 0usize;
        if let Some(first) = lines.first() {
            if first.starts_with("#!") {
                insert_line = 1;
            }
        }
        // Byte offset of the start of `insert_line`.
        let offset = lines
            .iter()
            .take(insert_line)
            .map(|l| l.len() + 1) // +1 for the '\n'
            .sum();
        Some((offset, new_text))
    } else {
        // Append after the last existing `use` line (newline-terminated already).
        let last_use = *use_lines.last().unwrap();
        let offset = lines
            .iter()
            .take(last_use + 1)
            .map(|l| l.len() + 1)
            .sum();
        Some((offset, new_text))
    }
}

/// Insert a `use <import_path>;` statement into `source`, reusing the existing
/// top-level `use` block when one is present. Idempotent: returns `source`
/// unchanged when the path is already imported.
pub fn insert_use(source: &str, import_path: &str) -> String {
    let Some((offset, new_text)) = insert_use_edit(source, import_path) else {
        return source.to_string();
    };
    let mut out = String::with_capacity(source.len() + new_text.len());
    out.push_str(&source[..offset]);
    out.push_str(&new_text);
    out.push_str(&source[offset..]);
    out
}

#[cfg(test)]
mod tests;
