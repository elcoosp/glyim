use glyim_diag::{FileId, Span};
use glyim_hir::types::HirType;
use glyim_hir::HirItem;
use glyim_interner::Interner;
use std::collections::HashMap;

/// A resolved symbol with its definition location and metadata.
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub definition: DefinitionLocation,
    pub type_signature: Option<TypeSignature>,
    pub is_pub: bool,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    EnumVariant,
    Field,
    TypeParameter,
    Local,
    Module,
}

#[derive(Debug, Clone)]
pub struct DefinitionLocation {
    pub file_id: FileId,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeSignature {
    pub params: Vec<(String, HirType)>,
    pub return_type: Option<HirType>,
}

/// Index for fast symbol lookup.
pub struct SymbolIndex {
    /// Name → list of symbols with that name.
    by_name: HashMap<String, Vec<SymbolInfo>>,
    /// All symbols in a file (for document symbol requests).
    by_file: HashMap<FileId, Vec<SymbolInfo>>,
    /// (file, start offset) → symbol
    by_location: HashMap<(u32, usize), SymbolInfo>,
}

impl Default for SymbolIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolIndex {
    pub fn new() -> Self {
        Self {
            by_name: HashMap::new(),
            by_file: HashMap::new(),
            by_location: HashMap::new(),
        }
    }

    /// Build the index from a HIR and interner.
    pub fn build_from_hir(
        &mut self,
        file_id: FileId,
        hir: &glyim_hir::Hir,
        interner: &Interner,
    ) {
        self.clear_file(file_id);
        let mut file_symbols = Vec::new();

        for item in &hir.items {
            match item {
                HirItem::Fn(f) => {
                    let name = interner.resolve(f.name).to_string();
                    let span = f.span;
                    let type_sig = TypeSignature {
                        params: f.params.iter().map(|(s, t)| {
                            (interner.resolve(*s).to_string(), t.clone())
                        }).collect(),
                        return_type: f.ret.clone(),
                    };
                    let doc = f.doc.clone();
                    let is_test = f.is_test;

                    let info = SymbolInfo {
                        name: name.clone(),
                        kind: SymbolKind::Function,
                        definition: DefinitionLocation { file_id, span },
                        type_signature: Some(type_sig),
                        is_pub: f.is_pub,
                        documentation: doc,
                    };
                    self.by_name.entry(name).or_default().push(info.clone());
                    self.by_location.insert((file_id.0, span.start), info.clone());
                    if !is_test {
                        file_symbols.push(info);
                    }
                }
                HirItem::Struct(s) => {
                    let name = interner.resolve(s.name).to_string();
                    let info = SymbolInfo {
                        name: name.clone(),
                        kind: SymbolKind::Struct,
                        definition: DefinitionLocation { file_id, span: s.span },
                        type_signature: None,
                        is_pub: s.is_pub,
                        documentation: s.doc.clone(),
                    };
                    self.by_name.entry(name).or_default().push(info.clone());
                    self.by_location.insert((file_id.0, s.span.start), info.clone());
                    file_symbols.push(info);
                }
                HirItem::Enum(e) => {
                    let name = interner.resolve(e.name).to_string();
                    let info = SymbolInfo {
                        name: name.clone(),
                        kind: SymbolKind::Enum,
                        definition: DefinitionLocation { file_id, span: e.span },
                        type_signature: None,
                        is_pub: e.is_pub,
                        documentation: e.doc.clone(),
                    };
                    self.by_name.entry(name).or_default().push(info.clone());
                    self.by_location.insert((file_id.0, e.span.start), info.clone());
                    file_symbols.push(info);
                }
                _ => {}
            }
        }
        self.by_file.insert(file_id, file_symbols);
    }

    /// Look up a symbol by name (returns all symbols with that name).
    pub fn lookup_by_name(&self, name: &str) -> Vec<&SymbolInfo> {
        self.by_name.get(name).map(|v| v.iter().collect()).unwrap_or_default()
    }

    /// Look up the symbol at a specific byte offset in a file.
    pub fn lookup_by_location(&self, file_id: FileId, offset: usize) -> Option<&SymbolInfo> {
        self.by_location.get(&(file_id.0, offset))
    }

    /// Get all symbols in a file (for document symbol request).
    pub fn symbols_in_file(&self, file_id: FileId) -> Vec<&SymbolInfo> {
        self.by_file.get(&file_id).map(|v| v.iter().collect()).unwrap_or_default()
    }

    /// Get all symbols matching a query prefix.
    pub fn query(&self, prefix: &str, limit: usize) -> Vec<&SymbolInfo> {
        let mut results = Vec::new();
        for (name, symbols) in &self.by_name {
            if name.starts_with(prefix) && results.len() < limit {
                for sym in symbols {
                    if results.len() < limit {
                        results.push(sym);
                    }
                }
            }
        }
        // Also match if prefix is contained (fuzzy)
        if results.is_empty() {
            for (name, symbols) in &self.by_name {
                if name.contains(prefix) && results.len() < limit {
                    for sym in symbols {
                        if results.len() < limit {
                            results.push(sym);
                        }
                    }
                }
            }
        }
        results
    }

    /// Clear the index for a specific file.
    /// Only for testing: insert a symbol directly into all indices.
    #[doc(hidden)]
    pub fn insert_test_symbol(&mut self, file_id: FileId, sym: SymbolInfo) {
        self.by_name.entry(sym.name.clone()).or_default().push(sym.clone());
        self.by_file.entry(file_id).or_default().push(sym.clone());
        self.by_location.insert((file_id.0, sym.definition.span.start), sym);
    }

    pub fn clear_file(&mut self, file_id: FileId) {
        if let Some(symbols) = self.by_file.remove(&file_id) {
            for sym in symbols {
                if let Some(entries) = self.by_name.get_mut(&sym.name) {
                    entries.retain(|s| s.definition.file_id != file_id);
                    if entries.is_empty() {
                        self.by_name.remove(&sym.name);
                    }
                }
                self.by_location.remove(&(file_id.0, sym.definition.span.start));
            }
        }
    }
}
