pub mod scope;
pub mod synth;
pub mod check;

use crate::ty::{Ty, TyKind};
use crate::unify::UnificationTable;
use crate::chr::ChrStore;
use crate::db::TyDatabase;
use crate::diagnostics::TypeError;
use crate::staging::Level;
use crate::elab::scope::Scope;
use glyim_hir::{HirItem, HirFn};
use std::collections::HashMap;

pub struct ElabContext<'a> {
    pub db: &'a mut TyDatabase,
    pub unification: &'a mut UnificationTable,
    pub chr_store: &'a mut ChrStore,
    pub current_level: Level,
    pub scope: Scope,
    pub expr_types: HashMap<glyim_hir::types::ExprId, Ty>,
    pub call_type_args: HashMap<glyim_hir::types::ExprId, Vec<Ty>>,
    pub errors: Vec<TypeError>,
    pub generated_items: Vec<glyim_hir::HirExpr>,
}

impl<'a> ElabContext<'a> {
    pub fn new(
        db: &'a mut TyDatabase,
        unification: &'a mut UnificationTable,
        chr_store: &'a mut ChrStore,
    ) -> Self {
        Self {
            db,
            unification,
            chr_store,
            current_level: Level::Runtime,
            scope: Scope::new(),
            expr_types: HashMap::new(),
            call_type_args: HashMap::new(),
            errors: Vec::new(),
            generated_items: Vec::new(),
        }
    }

    pub fn record_type(&mut self, id: glyim_hir::types::ExprId, ty: Ty) {
        self.expr_types.insert(id, ty);
    }

    pub fn emit_error(&mut self, err: TypeError) {
        self.errors.push(err);
    }

    pub fn elaborate_item(&mut self, item: &HirItem) {
        if let HirItem::Fn(def) = item {
            self.elaborate_fn(def);
        }
    }

    fn elaborate_fn(&mut self, def: &HirFn) {
        let ret_ty = self.hir_type_to_ty(
            def.ret.as_ref().unwrap_or(&glyim_hir::types::HirType::Int)
        );
        check::check_expr(self, &def.body, ret_ty);
    }

    fn hir_type_to_ty(&mut self, hir_type: &glyim_hir::types::HirType) -> Ty {
        match hir_type {
            glyim_hir::types::HirType::Int => self.db.arena.alloc(TyKind::Int),
            glyim_hir::types::HirType::Float => self.db.arena.alloc(TyKind::Float),
            glyim_hir::types::HirType::Bool => self.db.arena.alloc(TyKind::Bool),
            glyim_hir::types::HirType::Str => self.db.arena.alloc(TyKind::Str),
            glyim_hir::types::HirType::Unit => self.db.arena.alloc(TyKind::Unit),
            glyim_hir::types::HirType::Never => self.db.arena.alloc(TyKind::Never),
            glyim_hir::types::HirType::Error => self.db.arena.alloc(TyKind::Error),
            glyim_hir::types::HirType::Named(name) => {
                let name_copy = *name; // Symbol is Copy
                let name_str = self.db.interner.resolve(name_copy).to_string();
                // Drop immutable borrow before mutating
                drop(name);
                let sym = self.db.interner.intern(&name_str);
                self.db.arena.alloc(TyKind::Named(sym))
            }
            glyim_hir::types::HirType::Generic(name, args) => {
                let name_copy = *name;
                let name_str = self.db.interner.resolve(name_copy).to_string();
                drop(name);
                let sym = self.db.interner.intern(&name_str);
                let arg_tys: Vec<Ty> = args.iter().map(|a| self.hir_type_to_ty(a)).collect();
                self.db.arena.alloc(TyKind::App(sym, arg_tys))
            }
            glyim_hir::types::HirType::Func(params, ret) => {
                let param_tys: Vec<Ty> = params.iter().map(|p| self.hir_type_to_ty(p)).collect();
                let ret_ty = self.hir_type_to_ty(ret);
                self.db.arena.alloc(TyKind::Fn(param_tys, ret_ty))
            }
            glyim_hir::types::HirType::RawPtr(inner) => {
                let inner_ty = self.hir_type_to_ty(inner);
                self.db.arena.alloc(TyKind::RawPtr(inner_ty))
            }
            _ => self.db.arena.alloc(TyKind::Error),
        }
    }
}
