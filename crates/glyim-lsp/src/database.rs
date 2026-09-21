use crate::reference_graph::ReferenceGraph;
use crate::symbol_index::SymbolIndex;
use glyim_diag::GlyimDiagnostic;
use glyim_hir::Expr;
use glyim_span::FileId;
use glyim_type::Ty;
use glyim_typeck::TypeckResult;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
/// SourceMap.
pub struct SourceMap {
    #[allow(unused)]
    path: PathBuf,
    file_id: FileId,
    content: String,
    line_starts: Vec<usize>,
}

impl SourceMap {
    /// new.
    pub fn new(path: PathBuf, file_id: FileId, content: String) -> Self {
        let line_starts = std::iter::once(0)
            .chain(content.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self {
            path,
            file_id,
            content,
            line_starts,
        }
    }
    /// file_id.
    pub fn file_id(&self) -> FileId {
        self.file_id
    }
    /// source.
    pub fn source(&self) -> &str {
        &self.content
    }
    /// span_to_position.
    pub fn span_to_position(
        &self,
        lo: usize,
        hi: usize,
    ) -> Option<((usize, usize), (usize, usize))> {
        let start_line = self
            .line_starts
            .binary_search(&lo)
            .unwrap_or_else(|i| i - 1);
        let start_col = lo - self.line_starts[start_line];
        let end_line = self
            .line_starts
            .binary_search(&hi)
            .unwrap_or_else(|i| i - 1);
        let end_col = hi - self.line_starts[end_line];
        Some(((start_line, start_col), (end_line, end_col)))
    }
    /// line_col_to_offset.
    pub fn line_col_to_offset(&self, line: usize, col: usize) -> Option<usize> {
        if line >= self.line_starts.len() {
            return None;
        }
        let offset = self.line_starts[line] + col;
        if offset > self.content.len() {
            None
        } else {
            Some(offset)
        }
    }
}

/// FileMap.
pub struct FileMap {
    path_to_id: HashMap<PathBuf, FileId>,
    id_to_path: HashMap<FileId, PathBuf>,
    next_id: u32,
}

impl Default for FileMap {
    fn default() -> Self {
        Self::new()
    }
}

impl FileMap {
    /// new.
    pub fn new() -> Self {
        Self {
            path_to_id: HashMap::new(),
            id_to_path: HashMap::new(),
            next_id: 0,
        }
    }
    /// get_or_create.
    pub fn get_or_create(&mut self, path: &PathBuf) -> FileId {
        if let Some(id) = self.path_to_id.get(path) {
            return *id;
        }
        let id = FileId::from_raw(self.next_id);
        self.next_id += 1;
        self.path_to_id.insert(path.clone(), id);
        self.id_to_path.insert(id, path.clone());
        id
    }
    /// get_by_path.
    pub fn get_by_path(&self, path: &Path) -> Option<FileId> {
        self.path_to_id.get(path).copied()
    }
    /// path.
    pub fn path(&self, id: FileId) -> Option<&PathBuf> {
        self.id_to_path.get(&id)
    }
    /// remove.
    pub fn remove(&mut self, path: &PathBuf) {
        if let Some(id) = self.path_to_id.remove(path) {
            self.id_to_path.remove(&id);
        }
    }
}

/// AnalysisDatabase.
pub struct AnalysisDatabase {
    /// Struct.
    pub file_map: RwLock<FileMap>,
    /// Struct.
    pub source_maps: RwLock<HashMap<FileId, SourceMap>>,
    /// Struct.
    pub symbol_index: RwLock<SymbolIndex>,
    /// Struct.
    pub reference_graph: RwLock<ReferenceGraph>,
    /// Struct.
    pub hirs: RwLock<HashMap<FileId, glyim_hir::CrateHir>>,
    /// Per-file type-checking result + the `TyCtx` it was produced with.
    /// Populated by the analysis driver (Tier 6.4) so completions/hover can
    /// resolve the type of any expression via `expr_ty_at` /
    /// `type_at_offset`. Keyed by `FileId`.
    pub typeck: RwLock<HashMap<FileId, (Arc<glyim_type::TyCtx>, TypeckResult)>>,
    /// Struct.
    pub diagnostics: RwLock<HashMap<FileId, Vec<lsp_types::Diagnostic>>>,
    /// Struct.
    /// Raw `GlyimDiagnostic`s (preserving the `structured` payload) for each
    /// file, in 1:1 correspondence with `diagnostics` (same order). The LSP
    /// `Diagnostic` type cannot carry `structured`, so code actions that need it
    /// (e.g. the §5.1 match-arm skeleton quick-fix) read from here by index.
    pub raw_diagnostics: RwLock<HashMap<FileId, Vec<GlyimDiagnostic>>>,
    /// Struct.
    pub file_access_times: RwLock<HashMap<FileId, Instant>>,
}

impl Default for AnalysisDatabase {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisDatabase {
    /// new.
    pub fn new() -> Self {
        Self {
            file_map: RwLock::new(FileMap::new()),
            source_maps: RwLock::new(HashMap::new()),
            symbol_index: RwLock::new(SymbolIndex::new()),
            reference_graph: RwLock::new(ReferenceGraph::new()),
            hirs: RwLock::new(HashMap::new()),
            typeck: RwLock::new(HashMap::new()),
            diagnostics: RwLock::new(HashMap::new()),
            raw_diagnostics: RwLock::new(HashMap::new()),
            file_access_times: RwLock::new(HashMap::new()),
        }
    }

    /// touch.
    pub fn touch(&self, _file_id: FileId) {}
    /// evict_stale.
    pub fn evict_stale(&self, _max_age: std::time::Duration) {}

    /// Resolve the type of the HIR expression that contains `offset` in
    /// `file_id`. Used by completion (Tier 6.4) to filter by receiver type.
    ///
    /// When the cursor sits right after a `.` (a method-call / field-access
    /// completion trigger), we resolve the **receiver** of the enclosing
    /// `MethodCall`/`Field` expression rather than the innermost token at the
    /// dot — that receiver is what the `.` is being called on. Otherwise we
    /// fall back to the innermost expression whose span contains `offset` (or
    /// ends just before it, so a cursor placed right after a `.` still
    /// resolves the receiver).
    pub fn type_at_offset(&self, file_id: FileId, offset: usize) -> Option<Ty> {
        let hirs = self.hirs.read();
        let hir = hirs.get(&file_id)?;
        let typeck = self.typeck.read();
        let (_, result) = typeck.get(&file_id)?;

        // First pass: prefer the receiver of an enclosing MethodCall/Field
        // whose span contains the offset (the cursor is just after its `.`).
        let mut receiver_best: Option<(usize, glyim_core::def_id::LocalDefId, glyim_hir::ExprId)> =
            None;
        for (body_id, body) in hir.bodies.iter_enumerated() {
            for (expr_id, expr) in body.exprs.iter_enumerated() {
                let recv_id = match expr {
                    Expr::MethodCall { receiver, .. } | Expr::Field { receiver, .. } => *receiver,
                    _ => continue,
                };
                // The method-call / field-access span encloses the `.`, so the
                // cursor placed just after the `.` still falls inside it.
                let span = body.expr_spans.get(expr_id)?;
                let lo = span.lo.to_usize();
                let hi = span.hi.to_usize();
                if lo <= offset && offset <= hi {
                    let size = hi.saturating_sub(lo);
                    if receiver_best.map(|(b, _, _)| size < b).unwrap_or(true) {
                        let owner = hir.body_owners[body_id];
                        receiver_best = Some((size, owner, recv_id));
                    }
                }
            }
        }
        if let Some((_, owner, recv_id)) = receiver_best
            && let Some(ty) = result.expr_ty(owner, recv_id.to_raw() as usize)
        {
            return Some(ty);
        }

        // Fallback: innermost expression (by span) containing the offset, or
        // ending just before it.
        let mut best: Option<(usize, glyim_core::def_id::LocalDefId, glyim_hir::ExprId)> = None;
        for (body_id, body) in hir.bodies.iter_enumerated() {
            for (expr_id, span) in body.expr_spans.iter_enumerated() {
                let lo = span.lo.to_usize();
                let hi = span.hi.to_usize();
                let contains = lo <= offset && offset <= hi;
                let ends_before = hi == offset || hi == offset.saturating_sub(1);
                if contains || ends_before {
                    let size = hi.saturating_sub(lo);
                    if best.map(|(b, _, _)| size < b).unwrap_or(true) {
                        let owner = hir.body_owners[body_id];
                        best = Some((size, owner, expr_id));
                    }
                }
            }
        }
        let (_, owner, expr_id) = best?;
        result.expr_ty(owner, expr_id.to_raw() as usize)
    }

    /// Directly query a resolved expression type by `(LocalDefId, ExprId)`.
    pub fn expr_ty_at(
        &self,
        file_id: FileId,
        local_def_id: glyim_core::def_id::LocalDefId,
        expr_id: usize,
    ) -> Option<Ty> {
        let typeck = self.typeck.read();
        let (_, result) = typeck.get(&file_id)?;
        result.expr_ty(local_def_id, expr_id)
    }

    /// Access the frozen `TyCtx` for a file (for inspecting resolved `Ty`s).
    pub fn ty_ctx(&self, file_id: FileId) -> Option<Arc<glyim_type::TyCtx>> {
        self.typeck.read().get(&file_id).map(|(ctx, _)| ctx.clone())
    }
}
