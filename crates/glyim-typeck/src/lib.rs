pub mod ty;
pub mod unify;
pub mod diagnostics;
pub mod chr;
pub mod freeze;
pub mod staging;
pub mod elab;
pub mod rep;
pub mod reflect;
pub mod comptime;
pub mod queries;

use std::collections::HashMap;
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Interner;
use glyim_hir::Hir;
use crate::diagnostics::TypeError;
use crate::ty::TyArena;
use crate::unify::UnificationTable;
use crate::chr::ChrStore;
use crate::elab::ElabContext;

pub struct TypeCheckOutput {
    pub expr_types: Vec<HirType>,
    pub call_type_args: HashMap<ExprId, Vec<HirType>>,
    pub reflect_metadata: Vec<()>,
    pub generated_items: Vec<()>,
}

pub struct TypeChecker {
    interner: Interner,
}

impl TypeChecker {
    pub fn new(interner: Interner) -> Self {
        Self { interner }
    }

    pub fn check(&mut self, hir: &Hir) -> Result<TypeCheckOutput, Vec<TypeError>> {
        let mut arena = TyArena::new();
        let unification = UnificationTable::new();
        let chr_store = ChrStore::new(vec![]);

        let mut ctx = ElabContext::new(
            &mut arena,
            &mut self.interner,
            unification,
            chr_store,
            &hir.items,
        );

        for item in &hir.items {
            ctx.elaborate_item(item);
        }

        let errors = ctx.errors.clone();
        let elab_map = ctx.expr_types.clone();
        let call_type_args_raw = ctx.call_type_args.clone();
        let unification = ctx.unification;
        let _chr_store = ctx.chr_store;

        let expr_types = crate::freeze::resolve_expr_types(&arena, &unification, &elab_map);
        let call_type_args: HashMap<ExprId, Vec<HirType>> = {
            let mut map = HashMap::new();
            for (&id, tys) in &call_type_args_raw {
                let hir_tys: Vec<HirType> = tys.iter()
                    .map(|&ty| crate::freeze::resolve_ty(&arena, &unification, ty))
                    .collect();
                map.insert(id, hir_tys);
            }
            map
        };

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(TypeCheckOutput {
            expr_types,
            call_type_args,
            reflect_metadata: vec![],
            generated_items: vec![],
        })
    }
}

#[cfg(test)]
mod tests;
