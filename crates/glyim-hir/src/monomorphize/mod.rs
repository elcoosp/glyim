use crate::item::{HirItem, StructDef};
use crate::node::{HirExpr, HirFn, HirStmt};
use crate::types::{ExprId, HirType};
use glyim_interner::{Interner, Symbol};
use std::collections::{HashMap, HashSet};

/// Convert a HirType to a short string representation suitable for mangling.
/// e.g., Int → "i64", Named("Entry") → "Entry", Generic("Entry", [Int, Int]) → "Entry_i64_i64"
pub fn mangle_type_name(interner: &mut Interner, base: Symbol, type_args: &[HirType]) -> Symbol {
    let base_str = interner.resolve(base).to_string();
    let args_str = type_args
        .iter()
        .map(|t| type_to_short_string(t, interner))
        .collect::<Vec<_>>()
        .join("_");
    interner.intern(&format!("{}__{}", base_str, args_str))
}

pub struct MonoResult {
    pub hir: crate::Hir,
    pub type_overrides: HashMap<ExprId, HirType>,
}

#[tracing::instrument(skip_all)]
pub fn monomorphize(
    hir: &crate::Hir,
    interner: &mut Interner,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
) -> MonoResult {
    let mut ctx = MonoContext::new(hir, interner, expr_types, call_type_args);
    ctx.collect_and_specialize();
    ctx.build_result()
}


mod mangling;
pub use mangling::{type_to_short_string, mangle_type_name};

mod context;
mod collect;
mod specialize;
mod rewrite;
mod build_result;

#[cfg(test)]
mod tests;

struct MonoContext<'a> {
    hir: &'a crate::Hir,
    interner: &'a mut Interner,
    expr_types: &'a [HirType],
    call_type_args: &'a HashMap<ExprId, Vec<HirType>>,
    fn_specs: HashMap<(Symbol, Vec<HirType>), HirFn>,
    struct_specs: HashMap<(Symbol, Vec<HirType>), StructDef>,
    type_overrides: HashMap<ExprId, HirType>,
    fn_work_queue: Vec<(Symbol, Vec<HirType>)>,
    fn_queued: HashSet<(Symbol, Vec<HirType>)>,
    inferred_call_args: HashMap<ExprId, Vec<HirType>>,
    current_type_params: Vec<Symbol>,
}

