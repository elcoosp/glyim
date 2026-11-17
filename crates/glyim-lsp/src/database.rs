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

#[derive(Clone)]
/// `SourceMap`.
pub struct SourceMap {
    file_id: FileId,
    content: String,
    line_starts: Vec<usize>,
}

impl SourceMap {
    /// new.
    #[must_use]
    pub fn new(file_id: FileId, content: String) -> Self {
        let line_starts = std::iter::once(0)
            .chain(content.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self {
            file_id,
            content,
            line_starts,
        }
    }
    /// `file_id`.
    #[must_use]
    pub fn file_id(&self) -> FileId {
        self.file_id
    }
    /// source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.content
    }
    /// Byte offset -> `(line, utf16_col)` (INF-11). LSP columns are UTF-16
    /// code units, not bytes; the old code emitted byte columns, so every
    /// position after a non-ASCII character drifted.
    fn offset_to_line_utf16(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.content.len());
        let line = self
            .line_starts
            .binary_search(&offset)
            .unwrap_or_else(|i| i.saturating_sub(1));
        let line_start = self.line_starts[line];
        // Count UTF-16 units between the line start and `offset`.
        let col16 = self.content[line_start..offset]
            .chars()
            .map(char::len_utf16)
            .sum();
        (line, col16)
    }

    /// `span_to_position`.
    #[must_use]
    pub fn span_to_position(
        &self,
        lo: usize,
        hi: usize,
    ) -> Option<((usize, usize), (usize, usize))> {
        Some((self.offset_to_line_utf16(lo), self.offset_to_line_utf16(hi)))
    }

    /// `(line, utf16_col)` -> byte offset (INF-11). The column is a UTF-16
    /// code-unit count; it is converted to a byte offset on the line's char
    /// boundaries. Returns `None` if the column is past the end of the line
    /// (it never spills into the next line).
    #[must_use]
    pub fn line_col_to_offset(&self, line: usize, col: usize) -> Option<usize> {
        let start = *self.line_starts.get(line)?;
        // The line runs to the next newline (or EOF).
        let line_end = self.content[start..]
            .find('\n')
            .map_or(self.content.len(), |i| start + i);
        let line_text = &self.content[start..line_end];
        let mut u16_count = 0usize;
        for (i, ch) in line_text.char_indices() {
            if u16_count >= col {
                return Some(start + i);
            }
            u16_count += ch.len_utf16();
        }
        if u16_count == col {
            // Exactly end-of-line (before the newline).
            Some(line_end)
        } else {
            // Column past the end of the line.
            None
        }
    }
}

/// `FileMap`.
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
    #[must_use]
    pub fn new() -> Self {
        Self {
            path_to_id: HashMap::new(),
            id_to_path: HashMap::new(),
            next_id: 0,
        }
    }
    /// `get_or_create`.
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
    /// `get_by_path`.
    #[must_use]
    pub fn get_by_path(&self, path: &Path) -> Option<FileId> {
        self.path_to_id.get(path).copied()
    }
    /// path.
    #[must_use]
    pub fn path(&self, id: FileId) -> Option<&PathBuf> {
        self.id_to_path.get(&id)
    }
    /// remove.
    pub fn remove(&mut self, path: &PathBuf) {
        if let Some(id) = self.path_to_id.remove(path) {
            self.id_to_path.remove(&id);
        }
    }

    /// Re-point a path key to a new path, preserving the `FileId`
    /// (LSP-FILE-RENAME).
    ///
    /// Every recorded fact — source maps, symbol index, reference
    /// graph, HIR, parse trees, typeck state, recorded diagnostics — is
    /// keyed by the id, so preserving it is what makes the rename a
    /// *re-key*: the facts survive the move untouched, and only the two
    /// path maps move. Returns `false` without touching the map when
    /// the old path is unknown, or when the new path is already known —
    /// a collision must not orphan the existing path's facts, so a
    /// rename never half-applies.
    pub fn rename(&mut self, old: &Path, new: &Path) -> bool {
        let Some(id) = self.path_to_id.remove(old) else {
            return false;
        };
        if self.path_to_id.contains_key(new) {
            // Collision: put the old key back — the rename never
            // half-applies.
            self.path_to_id.insert(old.to_path_buf(), id);
            return false;
        }
        self.id_to_path.insert(id, new.to_path_buf());
        self.path_to_id.insert(new.to_path_buf(), id);
        true
    }
}

/// `AnalysisDatabase`.
pub struct AnalysisDatabase {
    /// `Struct`.
    pub file_map: RwLock<FileMap>,
    /// `Struct`.
    pub source_maps: RwLock<HashMap<FileId, SourceMap>>,
    /// `Struct`.
    pub symbol_index: RwLock<SymbolIndex>,
    /// `Struct`.
    pub reference_graph: RwLock<ReferenceGraph>,
    /// `Struct`.
    pub hirs: RwLock<HashMap<FileId, glyim_hir::CrateHir>>,
    /// Per-file parse trees, stored as the immutable `GreenNode` the
    /// analysis driver built (rowan's `SyntaxNode` cursor carries raw
    /// parent pointers and is not `Send`, but the green tree is plain
    /// shared data — a request materializes its own cursor root with one
    /// `Arc` clone). Populated next to `hirs` so syntax-tree-shaped
    /// requests walk real token extents without re-parsing. The
    /// selection-range engine (LSP-SELECTION) is the first consumer.
    pub syntaxes: RwLock<HashMap<FileId, glyim_syntax::GreenNode>>,
    /// Per-file type-checking result + the `TyCtx` it was produced with.
    /// Populated by the analysis driver (Tier 6.4) so completions/hover can
    /// resolve the type of any expression via `expr_ty_at` /
    /// `type_at_offset`. Keyed by `FileId`.
    pub typeck: RwLock<HashMap<FileId, (Arc<glyim_type::TyCtx>, TypeckResult)>>,
    /// `Struct`.
    pub diagnostics: RwLock<HashMap<FileId, Vec<lsp_types::Diagnostic>>>,
    /// `Struct`.
    /// Raw `GlyimDiagnostic`s (preserving the `structured` payload) for each
    /// file, in 1:1 correspondence with `diagnostics` (same order). The LSP
    /// `Diagnostic` type cannot carry `structured`, so code actions that need it
    /// (e.g. the §5.1 match-arm skeleton quick-fix) read from here by index.
    pub raw_diagnostics: RwLock<HashMap<FileId, Vec<GlyimDiagnostic>>>,
    /// Per-file client version (`textDocument.version` from `didOpen` /
    /// `didChange`, widened to `i64`) the analysis driver last analysed
    /// the file at. Populated by the driver next to the analysis
    /// artifacts and removed on close, so the workspace-diagnostics
    /// engine (LSP-WORKSPACE-DIAG) can answer each report with the
    /// honest `version` field: the recorded version, or `None` when the
    /// file was never opened by the client — never a fabricated 0.
    pub versions: RwLock<HashMap<FileId, i64>>,
    /// The inter-file dependency graph (path-keyed forward and reverse
    /// edges, `dep_graph::DependencyGraph`). This used to live as a
    /// private field on the analysis *driver* — the writer — which made
    /// it unreachable from the request path: the re-analysis enqueuing
    /// worked, but no request engine could ask "which files are affected
    /// by this one". The related-documents fan-out (LSP-RELATED-DIAG) is
    /// the first consumer; the driver still writes every edge and clears
    /// on close/re-analysis, exactly as before.
    pub dep_graph: RwLock<crate::dep_graph::DependencyGraph>,
    /// The paths the server loaded through the watched-files seam
    /// (LSP-WATCHED-FILES) and not through the editor. The watched-files
    /// engine distinguishes *editor-owned* documents (didOpen/didChange
    /// drive their content — a watched event for them is redundant) from
    /// session-known ones it loaded itself (a later watched event
    /// re-analyses them: external tools may touch them repeatedly).
    /// Recorded by the watched-files route when it re-analyses a path
    /// from disk, removed when the path is forgotten.
    pub watched_files: RwLock<std::collections::HashSet<PathBuf>>,
    /// The workspace folders the session serves (LSP-WORKSPACE-FOLDERS),
    /// in first-added order — seeded from the client's initialize params
    /// (the deprecated `rootUri` as the fallback), re-derived on
    /// `workspace/didChangeWorkspaceFolders`. Folders are session
    /// metadata keyed by the spelled path (the same keying the file map
    /// uses), so no analysis artifact hangs off this store; the folder
    /// queries (`session_folders`) read it through the
    /// `FolderSetSource` seam.
    pub workspace_folders: RwLock<Vec<crate::session_folders::FolderRecord>>,
}

impl Default for AnalysisDatabase {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisDatabase {
    /// new.
    #[must_use]
    pub fn new() -> Self {
        Self {
            file_map: RwLock::new(FileMap::new()),
            source_maps: RwLock::new(HashMap::new()),
            symbol_index: RwLock::new(SymbolIndex::new()),
            reference_graph: RwLock::new(ReferenceGraph::new()),
            hirs: RwLock::new(HashMap::new()),
            syntaxes: RwLock::new(HashMap::new()),
            typeck: RwLock::new(HashMap::new()),
            diagnostics: RwLock::new(HashMap::new()),
            raw_diagnostics: RwLock::new(HashMap::new()),
            versions: RwLock::new(HashMap::new()),
            dep_graph: RwLock::new(crate::dep_graph::DependencyGraph::new()),
            watched_files: RwLock::new(std::collections::HashSet::new()),
            workspace_folders: RwLock::new(Vec::new()),
        }
    }

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
                    if receiver_best.is_none_or(|(b, _, _)| size < b) {
                        // Translate the HIR BodyId through typeck's own
                        // counter (TCK-SEAM): the HIR and typeck
                        // LocalDefId counters are independent, so the raw
                        // HIR owner would resolve the wrong body.
                        let Some(owner) = result.body_owner(body_id) else {
                            continue;
                        };
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
                    if best.is_none_or(|(b, _, _)| size < b) {
                        // Same TCK-SEAM translation as the receiver pass.
                        let Some(owner) = result.body_owner(body_id) else {
                            continue;
                        };
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

#[cfg(test)]
mod source_map_tests {
    use super::SourceMap;
    use glyim_span::FileId;

    fn sm(content: &str) -> SourceMap {
        SourceMap::new(FileId::from_raw(1), content.to_string())
    }

    /// INF-11: LSP columns are UTF-16 code units. Round-trip on ASCII.
    #[test]
    fn round_trip_ascii() {
        let m = sm("let x = 5;\nlet y = x;\n");
        for (line, col) in [(0, 4), (0, 10), (1, 8)] {
            let off = m.line_col_to_offset(line, col).unwrap();
            let (l, c) = m.span_to_position(off, off).unwrap().0;
            assert_eq!((l, c), (line, col), "round trip failed for {line}:{col}");
        }
    }

    /// INF-11: after a multibyte char, byte-columns and UTF-16 columns differ.
    /// `é` is 1 UTF-16 unit but 2 bytes.
    #[test]
    fn multibyte_column_is_utf16() {
        let m = sm("aé b\n");
        // UTF-16 column 3 is the `b` (a=1, é=1, space=1, b at col 3).
        let off = m.line_col_to_offset(0, 3).unwrap();
        assert_eq!(&m.source()[off..=off], "b");
        // Byte offset of `b` is 4 (a=1, é=2, space=1).
        assert_eq!(off, 4);
        // And span_to_position maps that byte offset back to UTF-16 col 3.
        let (_, c) = m.span_to_position(off, off).unwrap().0;
        assert_eq!(c, 3);
    }

    /// A column past the end of the line is `None` (must not spill into the
    /// next line).
    #[test]
    fn column_past_eol_is_none() {
        let m = sm("ab\ncd\n");
        assert!(m.line_col_to_offset(0, 99).is_none());
    }
}
