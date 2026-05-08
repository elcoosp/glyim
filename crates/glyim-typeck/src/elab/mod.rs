pub mod scope;
pub mod synth;
pub mod check;

use crate::ty::{Ty, TyKind, TyArena};
use crate::unify::UnificationTable;
use crate::chr::ChrStore;
use crate::diagnostics::TypeError;
use crate::staging::Level;
use crate::elab::scope::Scope;
use glyim_hir::{HirItem, HirFn};
use glyim_interner::Interner;
use std::collections::HashMap;

pub struct ElabContext<'a> {
    pub arena: &'a mut TyArena,
    pub interner: &'a mut Interner,
    pub unification: UnificationTable,
    pub chr_store: ChrStore,
    pub current_level: Level,
    pub scope: Scope,
    pub expr_types: HashMap<glyim_hir::types::ExprId, Ty>,
    pub call_type_args: HashMap<glyim_hir::types::ExprId, Vec<Ty>>,
    pub errors: Vec<TypeError>,
    pub generated_items: Vec<glyim_hir::HirExpr>,
    /// The whole HIR for function lookups (type params, etc.)
    pub hir_items: &'a [HirItem],
}

impl<'a> ElabContext<'a> {
    pub fn new(
        arena: &'a mut TyArena,
        interner: &'a mut Interner,
        unification: UnificationTable,
        chr_store: ChrStore,
        hir_items: &'a [HirItem],
    ) -> Self {
        Self {
            arena,
            interner,
            unification,
            chr_store,
            current_level: Level::Runtime,
            scope: Scope::new(),
            expr_types: HashMap::new(),
            call_type_args: HashMap::new(),
            errors: Vec::new(),
            generated_items: Vec::new(),
            hir_items,
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
            glyim_hir::types::HirType::Int => self.arena.alloc(TyKind::Int),
            glyim_hir::types::HirType::Float => self.arena.alloc(TyKind::Float),
            glyim_hir::types::HirType::Bool => self.arena.alloc(TyKind::Bool),
            glyim_hir::types::HirType::Str => self.arena.alloc(TyKind::Str),
            glyim_hir::types::HirType::Unit => self.arena.alloc(TyKind::Unit),
            glyim_hir::types::HirType::Never => self.arena.alloc(TyKind::Never),
            glyim_hir::types::HirType::Error => self.arena.alloc(TyKind::Error),
            glyim_hir::types::HirType::Named(name) => {
                let name_str = self.interner.resolve(*name).to_string();
                let sym = self.interner.intern(&name_str);
                self.arena.alloc(TyKind::Named(sym))
            }
            glyim_hir::types::HirType::Generic(name, args) => {
                let name_str = self.interner.resolve(*name).to_string();
                let sym = self.interner.intern(&name_str);
                let arg_tys: Vec<Ty> = args.iter().map(|a| self.hir_type_to_ty(a)).collect();
                self.arena.alloc(TyKind::App(sym, arg_tys))
            }
            glyim_hir::types::HirType::Func(params, ret) => {
                let param_tys: Vec<Ty> = params.iter().map(|p| self.hir_type_to_ty(p)).collect();
                let ret_ty = self.hir_type_to_ty(ret);
                self.arena.alloc(TyKind::Fn(param_tys, ret_ty))
            }
            glyim_hir::types::HirType::RawPtr(inner) => {
                let inner_ty = self.hir_type_to_ty(inner);
                self.arena.alloc(TyKind::RawPtr(inner_ty))
            }
            _ => self.arena.alloc(TyKind::Error),
        }
    }
}
