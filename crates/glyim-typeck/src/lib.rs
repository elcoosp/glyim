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
pub mod db;

use std::collections::HashMap;
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Interner;
use glyim_hir::Hir;
use crate::diagnostics::TypeError;

/// The output expected by `glyim-compiler::pipeline`.
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

    pub fn check(
        &mut self,
        hir: &Hir,
    ) -> Result<TypeCheckOutput, Vec<TypeError>> {
        let mut db = crate::db::TyDatabase::new(self.interner.clone());
        db.check_module(hir)
    }
}

#[cfg(test)]
mod tests;
