use crate::ty::{Ty, TyArena};
use crate::chr::ChrStore;
use crate::unify::UnificationTable;
use crate::freeze;
use crate::diagnostics::TypeError;
use crate::elab::ElabContext;
use glyim_hir::Hir;
use glyim_interner::Interner;
use std::collections::HashMap;
use crate::TypeCheckOutput;

pub struct TyDatabase {
    pub arena: TyArena,
    pub interner: Interner,
}

impl TyDatabase {
    pub fn new(interner: Interner) -> Self {
        Self {
            arena: TyArena::new(),
            interner,
        }
    }

    pub fn check_module(&mut self, hir: &Hir) -> Result<TypeCheckOutput, Vec<TypeError>> {
        let mut chr_store = ChrStore::new(vec![]);
        let mut unification = UnificationTable::new();
        let mut ctx = ElabContext::new(self, &mut unification, &mut chr_store);
        for item in &hir.items {
            ctx.elaborate_item(item);
        }
        let errors = ctx.errors.clone();
        let elab_map = ctx.expr_types.clone();
        let call_type_args_raw = ctx.call_type_args.clone();
        drop(ctx);

        if let Err(_) = chr_store.solve(&self.arena) {
            // errors already accumulated
        }

        let expr_types = freeze::resolve_expr_types(&self.arena, &unification, &elab_map);
        let call_type_args: HashMap<glyim_hir::types::ExprId, Vec<glyim_hir::types::HirType>> = {
            let mut map = HashMap::new();
            for (&id, tys) in &call_type_args_raw {
                let hir_tys: Vec<glyim_hir::types::HirType> = tys.iter()
                    .map(|&ty| freeze::resolve_ty(&self.arena, &unification, ty))
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
