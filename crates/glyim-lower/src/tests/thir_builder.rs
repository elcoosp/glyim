use glyim_core::Name;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::interner::Interner;
use glyim_core::primitives::Mutability;
use glyim_span::Span;
use glyim_type::Ty;
use glyim_typeck::thir;
use std::collections::HashMap;

/// Helper to build THIR expressions directly.
pub struct ThirBuilder {
    pub return_ty: Ty,
    pub owner: DefId,
    interner: Interner,
    var_counter: u32,
    pub var_names: HashMap<Name, thir::LocalVarId>,
}

impl ThirBuilder {
    pub fn new(return_ty: Ty, interner: Interner) -> Self {
        Self {
            return_ty,
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            interner,
            var_counter: 0,
            var_names: HashMap::new(),
        }
    }

    pub fn make_name(&self, name: &str) -> Name {
        self.interner.intern(name)
    }

    pub fn expr(&self, kind: thir::ExprKind, ty: Ty) -> thir::Expr {
        thir::Expr {
            kind,
            ty,
            span: Span::DUMMY,
        }
    }

    pub fn pat(&self, kind: thir::PatternKind, ty: Ty) -> thir::Pattern {
        thir::Pattern {
            kind,
            ty,
            span: Span::DUMMY,
        }
    }

    pub fn add_let_binding(
        &mut self,
        name: &str,
        ty: Ty,
        init: Option<thir::Expr>,
        stmts: &mut Vec<thir::Stmt>,
    ) {
        let n = self.make_name(name);
        let var_id = thir::LocalVarId::from_raw(self.var_counter);
        self.var_counter += 1;
        self.var_names.insert(n, var_id);
        let pat = self.pat(
            thir::PatternKind::Binding {
                var_id: thir::LocalVarId::from_raw(0),
                name: n,
                mutability: Mutability::Not,
                binding_ty: None,
                subpattern: None,
            },
            ty,
        );
        stmts.push(thir::Stmt::Let {
            name: n,
            ty,
            pat,
            init,
            span: Span::DUMMY,
        });
    }

    pub fn var_ref_expr(&self, name: &str, ty: Ty) -> thir::Expr {
        let _sym = self.interner.intern(name);
        let n = self.interner.intern(name);
        let var_id = *self.var_names.get(&n).expect("var not found");
        thir::Expr {
            kind: thir::ExprKind::VarRef(var_id),
            ty,
            span: Span::DUMMY,
        }
    }

    pub fn into_body(self, stmts: Vec<thir::Stmt>, params: Vec<thir::Param>) -> thir::Body {
        thir::Body {
            owner: self.owner,
            params,
            return_ty: self.return_ty,
            stmts,
            span: Span::DUMMY,
        }
    }
}

// Helper to create a MatchArm
pub fn match_arm(pat: thir::Pattern, body: thir::Expr) -> thir::MatchArm {
    thir::MatchArm {
        pat,
        guard: None,
        body,
    }
}
