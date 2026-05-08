use glyim_diag::{FileId, Span};
use glyim_hir::{HirExpr, HirItem, HirStmt};
use glyim_interner::Interner;
use std::collections::HashMap;

/// A reference to a symbol at a specific source location.
#[derive(Debug, Clone)]
pub struct Reference {
    pub file_id: FileId,
    pub span: Span,
    pub is_definition: bool,
    pub kind: ReferenceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    Call,
    TypeReference,
    FieldAccess,
    Constructor,
    Pattern,
}

/// Maps symbol names to all locations where they are referenced.
pub struct ReferenceGraph {
    references: HashMap<String, Vec<Reference>>,
}

impl Default for ReferenceGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ReferenceGraph {
    pub fn new() -> Self {
        Self {
            references: HashMap::new(),
        }
    }

    /// Build the reference graph from a HIR.
    pub fn build_from_hir(&mut self, file_id: FileId, hir: &glyim_hir::Hir, interner: &Interner) {
        // Remove old entries for this file
        self.references.retain(|_, refs| {
            refs.retain(|r| r.file_id != file_id);
            !refs.is_empty()
        });

        for item in &hir.items {
            if let HirItem::Fn(f) = item {
                let fn_name = interner.resolve(f.name).to_string();
                // Definition
                self.references.entry(fn_name.clone()).or_default().push(Reference {
                    file_id,
                    span: f.span,
                    is_definition: true,
                    kind: ReferenceKind::Call,
                });
                // Walk body for references
                self.collect_expr_refs(file_id, &f.body, interner);
            }
        }
    }

    fn collect_expr_refs(&mut self, file_id: FileId, expr: &HirExpr, interner: &Interner) {
        match expr {
            HirExpr::Call { callee, args, span, .. } => {
                let name = interner.resolve(*callee).to_string();
                self.references.entry(name).or_default().push(Reference {
                    file_id,
                    span: *span,
                    is_definition: false,
                    kind: ReferenceKind::Call,
                });
                for a in args {
                    self.collect_expr_refs(file_id, a, interner);
                }
            }
            HirExpr::FieldAccess { object, field, span, .. } => {
                let name = interner.resolve(*field).to_string();
                self.references.entry(name).or_default().push(Reference {
                    file_id,
                    span: *span,
                    is_definition: false,
                    kind: ReferenceKind::FieldAccess,
                });
                self.collect_expr_refs(file_id, object, interner);
            }
            HirExpr::Block { stmts, .. } => {
                for stmt in stmts {
                    match stmt {
                        HirStmt::Expr(e) | HirStmt::Let { value: e, .. } | HirStmt::Assign { value: e, .. } => {
                            self.collect_expr_refs(file_id, e, interner);
                        }
                        _ => {}
                    }
                }
            }
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.collect_expr_refs(file_id, condition, interner);
                self.collect_expr_refs(file_id, then_branch, interner);
                if let Some(eb) = else_branch {
                    self.collect_expr_refs(file_id, eb, interner);
                }
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.collect_expr_refs(file_id, lhs, interner);
                self.collect_expr_refs(file_id, rhs, interner);
            }
            _ => {}
        }
    }

    
    /// Only for testing: insert a reference directly.
    #[doc(hidden)]
    pub fn insert_test_reference(&mut self, name: &str, reference: Reference) {
        self.references.entry(name.to_string()).or_default().push(reference);
    }

    pub fn find_references(&self, symbol_name: &str) -> &[Reference] {
        self.references.get(symbol_name).map(|v| v.as_slice()).unwrap_or(&[])
    }
}
