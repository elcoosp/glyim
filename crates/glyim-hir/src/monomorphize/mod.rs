//! Monomorphization: convert generic HIR into fully concrete HIR.

pub mod concretize;
pub mod context;
pub mod discover;
pub mod index;
pub mod mangle_table;
pub mod mangling;
pub mod pattern;
pub mod specialize;
pub mod subst;
pub mod work;

pub use context::MonoResult;
pub use mangling::{mangle_type_name, type_to_short_string};

use crate::types::{ExprId, HirType};
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

#[tracing::instrument(skip_all)]
pub fn monomorphize(
    hir: &crate::Hir,
    interner: &mut Interner,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
) -> MonoResult {
    let main_sym = interner.intern("main");
    monomorphize_with_entries(hir, interner, expr_types, call_type_args, &[main_sym])
}

#[tracing::instrument(skip_all)]
pub fn monomorphize_with_entries(
    hir: &crate::Hir,
    interner: &mut Interner,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
    entry_points: &[Symbol],
) -> MonoResult {
    let mut ctx = context::MonoContext::new(interner, expr_types, call_type_args, hir);
    ctx.run(hir, entry_points);
    ctx.into_result()
}

#[cfg(test)]
mod tests;
