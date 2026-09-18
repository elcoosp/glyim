use glyim_core::{Interner, LocalDefId};
use glyim_hir::{Body, CrateHir, ItemKind, Pat, PatId};
use glyim_span::{FileId, Span};
use std::collections::HashMap;

#[derive(Debug, Clone)]
/// SymbolInfo.
pub struct SymbolInfo {
/// Struct.
    pub name: String,
/// Struct.
    pub kind: SymbolKind,
/// Struct.
    pub definition: DefinitionLocation,
/// Struct.
    pub type_signature: Option<TypeSignature>,
/// Struct.
    pub is_pub: bool,
/// Struct.
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// SymbolKind.
pub enum SymbolKind {
/// Variant.
    Function,
/// Variant.
    Struct,
/// Variant.
    Enum,
/// Variant.
    EnumVariant,
/// Variant.
    Field,
/// Variant.
    TypeParameter,
/// Variant.
    Local,
/// Variant.
    Module,
}

#[derive(Debug, Clone)]
/// DefinitionLocation.
pub struct DefinitionLocation {
/// Struct.
    pub file_id: FileId,
/// Struct.
    pub span: Span,
}

#[derive(Debug, Clone)]
/// TypeSignature.
pub struct TypeSignature {
#[doc = "field"]
    pub params: Vec<(String, String)>,
/// Struct.
    pub return_type: Option<String>,
    /// For method symbols, the resolved receiver (`self`) type, e.g. `"Foo"`.
    /// `None` for free functions / item symbols. Used by Tier 6.4 completion
    /// filtering to match a `.`-method call's receiver type.
    pub receiver_type: Option<String>,
    /// Names of the type parameters of this item (e.g. `["T", "U"]` for
    /// `fn f<T, U>(...)`). Used by completion (plan §22.6) to emit a generic
    /// snippet `f::<${1:T}, ${2:U}>(...)`. Lifetime/const params are excluded
    /// — they are not elidable in a call-site snippet the way type params are.
    pub generic_params: Vec<String>,
}

/// Collect the names of the *type* parameters of a generic declaration.
/// Lifetime and const generic parameters are intentionally excluded.
fn type_param_names(params: &[glyim_hir::GenericParam], interner: &Interner) -> Vec<String> {
    params
        .iter()
        .filter_map(|p| match p.kind {
            glyim_hir::GenericParamKind::Type { .. } => {
                Some(interner.resolve(p.name).to_string())
            }
            _ => None,
        })
        .collect()
}

/// SymbolIndex.
pub struct SymbolIndex {
    by_name: HashMap<String, Vec<SymbolInfo>>,
    by_file: HashMap<FileId, Vec<SymbolInfo>>,
    by_location: HashMap<(u32, usize), SymbolInfo>,
    /// Plan §22.6 auto-import: maps `(file_id, symbol_name)` -> fully-qualified
    /// import path (e.g. `crate::foo::Bar`) so completion can offer to insert a
    /// `use` statement for symbols declared in *other* files.
    import_paths: HashMap<(u32, String), String>,
}

impl Default for SymbolIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolIndex {
/// new.
    pub fn new() -> Self {
        Self {
            by_name: HashMap::new(),
            by_file: HashMap::new(),
            by_location: HashMap::new(),
            import_paths: HashMap::new(),
        }
    }

/// build_from_hir.
    pub fn build_from_hir(&mut self, file_id: FileId, hir: &CrateHir, interner: &Interner) {
        self.clear_file(file_id);

        // Plan §22.6 auto-import: build a parent map (`child -> parent module
        // item`) so each item's fully-qualified import path can be derived, then
        // record it under `(file_id, name)` for completion to look up later.
        let mut parent: HashMap<glyim_hir::ItemId, glyim_hir::ItemId> = HashMap::new();
        for it in hir.items.iter() {
            if let ItemKind::Mod(m) = &it.kind {
                for &child in &m.children {
                    parent.insert(child, it.id);
                }
            }
        }
        for it in hir.items.iter() {
            let path = Self::module_path_of(it.id, &parent, hir, interner);
            self.import_paths
                .insert((file_id.to_raw(), interner.resolve(it.name).to_string()), path);
        }

        for item in hir.items.iter() {
            let name = interner.resolve(item.name).to_string();

            // Tier 6.4: index each method of an `impl` block as a function
            // symbol carrying its receiver (`self`) type, so completion can
            // filter `.`-method calls by receiver type.
            if let ItemKind::Impl(impl_item) = &item.kind {
                let receiver_str = render_type_ref(&impl_item.self_ty, interner);
                let def_loc = DefinitionLocation {
                    file_id,
                    span: item.span,
                };
                for method in &impl_item.methods {
                    let params: Vec<(String, String)> = method
                        .params
                        .iter()
                        .map(|p| {
                            let ty_str =
                                p.ty.as_ref()
                                    .map(|t| render_type_ref(t, interner))
                                    .unwrap_or_else(|| "unknown".to_string());
                            (interner.resolve(p.name).to_string(), ty_str)
                        })
                        .collect();
                    let return_ty = method
                        .return_ty
                        .as_ref()
                        .map(|t| render_type_ref(t, interner));
                    let generics = type_param_names(&impl_item.generic_params, interner);
                    let info = SymbolInfo {
                        name: interner.resolve(method.name).to_string(),
                        kind: SymbolKind::Function,
                        definition: def_loc.clone(),
                        type_signature: Some(TypeSignature {
                            params,
                            return_type: return_ty,
                            receiver_type: Some(receiver_str.clone()),
                            generic_params: generics,
                        }),
                        is_pub: matches!(item.visibility, glyim_core::Visibility::Public),
                        documentation: None,
                    };
                    self.insert_symbol(file_id, info);
                }
                continue;
            }

            let kind = match item.kind {
                ItemKind::Fn(_) => SymbolKind::Function,
                ItemKind::Struct(_) => SymbolKind::Struct,
                ItemKind::Enum(_) => SymbolKind::Enum,
                _ => continue,
            };
            let span = item.span;
            let def_loc = DefinitionLocation { file_id, span };
            let type_sig = match &item.kind {
                ItemKind::Fn(fn_item) => {
                    let params: Vec<(String, String)> = fn_item
                        .params
                        .iter()
                        .map(|p| {
                            let ty_str =
                                p.ty.as_ref()
                                    .map(|t| render_type_ref(t, interner))
                                    .unwrap_or_else(|| "unknown".to_string());
                            (interner.resolve(p.name).to_string(), ty_str)
                        })
                        .collect();
                    let return_ty = fn_item
                        .return_ty
                        .as_ref()
                        .map(|t| render_type_ref(t, interner));
                    Some(TypeSignature {
                        params,
                        return_type: return_ty,
                        receiver_type: None,
                        generic_params: type_param_names(&fn_item.generic_params, interner),
                    })
                }
                ItemKind::Struct(struct_item) => {
                    let fields: Vec<(String, String)> = struct_item
                        .fields
                        .iter()
                        .map(|f| {
                            let ty_str = format!("{:?}", f.ty);
                            (interner.resolve(f.name).to_string(), ty_str)
                        })
                        .collect();
                    Some(TypeSignature {
                        params: fields,
                        return_type: None,
                        receiver_type: None,
                        generic_params: type_param_names(&struct_item.generic_params, interner),
                    })
                }
                ItemKind::Enum(enum_item) => {
                    let variants: Vec<(String, String)> = enum_item
                        .variants
                        .iter()
                        .map(|v| {
                            let fields_str = if v.fields.is_empty() {
                                String::new()
                            } else {
                                let tys: Vec<String> =
                                    v.fields.iter().map(|f| format!("{:?}", f.ty)).collect();
                                format!("({})", tys.join(", "))
                            };
                            (interner.resolve(v.name).to_string(), fields_str)
                        })
                        .collect();
                    Some(TypeSignature {
                        params: variants,
                        return_type: None,
                        receiver_type: None,
                        generic_params: type_param_names(&enum_item.generic_params, interner),
                    })
                }
                _ => None,
            };
            let is_pub = matches!(item.visibility, glyim_core::Visibility::Public);
            let info = SymbolInfo {
                name: name.clone(),
                kind,
                definition: def_loc,
                type_signature: type_sig,
                is_pub,
                documentation: None,
            };
            self.insert_symbol(file_id, info);
        }

        for (body_id, body) in hir.bodies.iter_enumerated() {
            let owner = hir.body_owners[body_id];
            self.index_body(file_id, body, interner, owner);
        }
    }

    fn index_body(
        &mut self,
        file_id: FileId,
        body: &Body,
        interner: &Interner,
        _owner: LocalDefId,
    ) {
        for (pat_id, pat) in body.pats.iter_enumerated() {
            self.index_pattern(file_id, pat_id, pat, body, interner);
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn index_pattern(
        &mut self,
        file_id: FileId,
        _pat_id: PatId,
        pat: &Pat,
        body: &Body,
        interner: &Interner,
    ) {
        match pat {
            Pat::Binding {
                name,
                mutability: _,
                subpattern,
            } => {
                let name_str = interner.resolve(*name).to_string();
                // HIR does not store spans for patterns; use DUMMY for now
                let span = Span::DUMMY;
                let def_loc = DefinitionLocation { file_id, span };
                let info = SymbolInfo {
                    name: name_str,
                    kind: SymbolKind::Local,
                    definition: def_loc,
                    type_signature: None,
                    is_pub: false,
                    documentation: None,
                };
                self.insert_symbol(file_id, info);
                if let Some(sub) = subpattern {
                    self.index_pattern(file_id, *sub, pat, body, interner);
                }
            }
            Pat::Struct {
                path: _,
                fields,
                rest: _,
            } => {
                for (_, field_pat) in fields {
                    self.index_pattern(file_id, *field_pat, pat, body, interner);
                }
            }
            Pat::Tuple(pats) | Pat::Or(pats) => {
                for p in pats {
                    self.index_pattern(file_id, *p, pat, body, interner);
                }
            }
            _ => {}
        }
    }

    fn insert_symbol(&mut self, file_id: FileId, info: SymbolInfo) {
        let name = info.name.clone();
        self.by_name.entry(name).or_default().push(info.clone());
        self.by_file.entry(file_id).or_default().push(info.clone());
        self.by_location
            .insert((file_id.to_raw(), info.definition.span.lo.to_usize()), info);
    }

/// lookup_by_name.
    pub fn lookup_by_name(&self, name: &str) -> Vec<&SymbolInfo> {
        self.by_name
            .get(name)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

/// lookup_by_location.
    pub fn lookup_by_location(&self, file_id: FileId, offset: usize) -> Option<&SymbolInfo> {
        // A hover/click position falls *somewhere inside* a symbol's definition
        // span, not necessarily on its start byte. Match by span containment
        // (lo <= offset < hi) rather than an exact start-offset key, so any
        // position within the symbol resolves to it.
        self.by_file
            .get(&file_id)?
            .iter()
            .find(|sym| {
                let lo = sym.definition.span.lo.to_usize();
                let hi = sym.definition.span.hi.to_usize();
                offset >= lo && offset < hi
            })
    }

/// symbols_in_file.
    pub fn symbols_in_file(&self, file_id: FileId) -> Vec<&SymbolInfo> {
        self.by_file
            .get(&file_id)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

/// query.
    pub fn query(&self, prefix: &str, limit: usize) -> Vec<&SymbolInfo> {
        // Tiered matching (plan §22.3): exact > prefix > contains > fuzzy
        // subsequence. Exact/prefix/contains stay the fast paths; fuzzy is a
        // final fallback so common lookups stay predictable, while subsequence
        // typos (e.g. `gsrbt` -> `get_something_related_by_type`) still surface.
        if prefix.is_empty() {
            return self
                .by_name
                .values()
                .flat_map(|v| v.iter())
                .take(limit)
                .collect();
        }

        let mut exact = Vec::new();
        let mut prefix_matches = Vec::new();
        let mut contains = Vec::new();
        let mut fuzzy: Vec<(usize, &SymbolInfo)> = Vec::new();

        for (name, symbols) in &self.by_name {
            if name == prefix {
                exact.extend(symbols.iter());
            } else if name.starts_with(prefix) {
                prefix_matches.extend(symbols.iter());
            } else if name.contains(prefix) {
                contains.extend(symbols.iter());
            } else if let Some(score) = Self::fuzzy_score(prefix, name) {
                fuzzy.extend(symbols.iter().map(|s| (score, s)));
            }
        }

        let mut results: Vec<&SymbolInfo> = Vec::new();
        results.extend(exact.iter().take(limit.saturating_sub(results.len())));
        results.extend(
            prefix_matches
                .iter()
                .take(limit.saturating_sub(results.len())),
        );
        results.extend(
            contains
                .iter()
                .take(limit.saturating_sub(results.len())),
        );
        if results.len() < limit {
            fuzzy.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
            results.extend(
                fuzzy
                    .iter()
                    .map(|(_, s)| *s)
                    .take(limit.saturating_sub(results.len())),
            );
        }
        results
    }

    /// Plan §22.6 auto-import: return the fully-qualified import path recorded
    /// for `name` in `file_id` (e.g. `crate::foo::Bar`), if indexed.
    pub fn import_path_for(&self, file_id: FileId, name: &str) -> Option<&String> {
        self.import_paths
            .get(&(file_id.to_raw(), name.to_string()))
    }

/// Plan §22.6 auto-import: derive a fully-qualified import path for `id` by
/// walking up the module-parent chain (built in `build_from_hir`). An item at
/// the crate root yields its bare name; a nested item yields
/// `crate::mod1::mod2::Name`.
fn module_path_of(
    id: glyim_hir::ItemId,
    parent: &HashMap<glyim_hir::ItemId, glyim_hir::ItemId>,
    hir: &glyim_hir::CrateHir,
    interner: &glyim_core::Interner,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = Some(id);
    while let Some(c) = cur {
        let item = &hir.items[c];
        parts.push(interner.resolve(item.name).to_string());
        cur = parent.get(&c).copied();
    }
    parts.reverse();
    if parts.len() <= 1 {
        parts.join("::")
    } else {
        format!("crate::{}", parts.join("::"))
    }
}

/// Subsequence-based fuzzy scorer (plan §22.3), Sublime-Text-style:
/// - requires every character of `query` to appear in order in `candidate`;
/// - consecutive matches and word-boundary (after `_`/start) matches score higher;
/// - returns `None` when `query` is not a subsequence of `candidate`.
fn fuzzy_score(query: &str, candidate: &str) -> Option<usize> {
    if query.is_empty() || query.len() > candidate.len() {
        return None;
    }
    let q: Vec<char> = query.chars().collect();
    let c: Vec<char> = candidate.chars().collect();
    let mut qi = 0usize;
    let mut prev_ci = None::<usize>;
    let mut score = 0usize;
    let mut is_first = true;
    for (ci, &cc) in c.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        if cc != q[qi] {
            continue;
        }
        score += 1;
        if let Some(pci) = prev_ci {
            if pci + 1 == ci {
                score += 4;
            }
        }
        let at_boundary = is_first || ci == 0 || c[ci - 1] == '_';
        if at_boundary {
            score += 3;
        }
        prev_ci = Some(ci);
        is_first = false;
        qi += 1;
    }
    if qi == q.len() {
        Some(score * 100 + (c.len() - q.len()))
    } else {
        None
    }
}

/// clear_file.
    pub fn clear_file(&mut self, file_id: FileId) {
        if let Some(symbols) = self.by_file.remove(&file_id) {
            for sym in symbols {
                if let Some(entries) = self.by_name.get_mut(&sym.name) {
                    entries.retain(|s| s.definition.file_id != file_id);
                    if entries.is_empty() {
                        self.by_name.remove(&sym.name);
                    }
                }
                self.by_location
                    .remove(&(file_id.to_raw(), sym.definition.span.lo.to_usize()));
            }
        }
        self.import_paths.retain(|(fid, _), _| *fid != file_id.to_raw());
    }

    #[doc(hidden)]
    pub fn insert_test_symbol(&mut self, file_id: FileId, sym: SymbolInfo) {
        self.insert_symbol(file_id, sym);
    }

    /// Test-only: record an import path for `(file_id, name)` so auto-import
    /// completion tests can look it up via `import_path_for`.
    #[doc(hidden)]
    pub fn insert_test_import_path(&mut self, file_id: FileId, name: &str, path: &str) {
        self.import_paths
            .insert((file_id.to_raw(), name.to_string()), path.to_string());
    }
}

/// Render a HIR `TypeRef` to a human-readable type name string.
///
/// Used to record an impl method's receiver type (`SymbolInfo::receiver_type`)
/// in a form comparable with the resolved receiver type produced by
/// `AnalysisDatabase::type_at_offset` (which renders a `Ty` via `PrintTy`).
pub fn render_type_ref(ty: &glyim_hir::TypeRef, interner: &glyim_core::Interner) -> String {
    match ty {
        glyim_hir::TypeRef::Path(path) => path
            .segments
            .last()
            .map(|s| interner.resolve(s.name).to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        glyim_hir::TypeRef::Ref { inner, mutability } => {
            let prefix = if matches!(mutability, glyim_core::primitives::Mutability::Mut) {
                "&mut "
            } else {
                "&"
            };
            format!("{}{}", prefix, render_type_ref(inner, interner))
        }
        glyim_hir::TypeRef::RawPtr { inner, mutability } => {
            let prefix = if matches!(mutability, glyim_core::primitives::Mutability::Mut) {
                "*mut "
            } else {
                "*const "
            };
            format!("{}{}", prefix, render_type_ref(inner, interner))
        }
        glyim_hir::TypeRef::Slice(inner) => format!("[{}]", render_type_ref(inner, interner)),
        glyim_hir::TypeRef::Array { inner, .. } => {
            format!("[{}]", render_type_ref(inner, interner))
        }
        glyim_hir::TypeRef::Tuple(tys) => {
            let inner: Vec<String> = tys.iter().map(|t| render_type_ref(t, interner)).collect();
            format!("({})", inner.join(", "))
        }
        glyim_hir::TypeRef::Fn { params, ret } => {
            let ps: Vec<String> = params
                .iter()
                .map(|t| render_type_ref(t, interner))
                .collect();
            let r = ret
                .as_ref()
                .map(|t| render_type_ref(t, interner))
                .unwrap_or_default();
            format!("fn({}) -> {}", ps.join(", "), r)
        }
        glyim_hir::TypeRef::Never => "!".to_string(),
        glyim_hir::TypeRef::Infer => "_".to_string(),
        glyim_hir::TypeRef::Dyn(inner) => {
            format!("dyn {}", render_type_ref(inner, interner))
        }
        glyim_hir::TypeRef::Error => "error".to_string(),
    }
}
