// crates/glyim-hir/src/monomorphize/mod.rs
use crate::item::StructDef;
use crate::node::HirFn;
use crate::types::{ExprId, HirType};
use glyim_interner::{Interner, Symbol};
use std::collections::{HashMap, HashSet};

pub mod mangling;
pub use mangling::{mangle_type_name, type_to_short_string};

mod build_result;
mod collect;
mod context;
mod rewrite;
mod specialize;

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
    eprintln!(
        "[mono] entering monomorphize with call_type_args: {:?}",
        call_type_args
    );
    ctx.collect_and_specialize();
    eprintln!(
        "[mono] Interner symbol map ({} entries):",
        ctx.interner.len()
    );
    for i in 0..ctx.interner.len() {
        if let Some(sym) = ctx.interner.get_symbol(i as u32) {
            eprintln!("  Symbol({}) = {:?}", i, ctx.interner.resolve(sym));
        }
    }
    ctx.build_result()
}

pub(crate) struct MonoContext<'a> {
    pub(crate) hir: &'a crate::Hir,
    pub(crate) interner: &'a mut Interner,
    pub(crate) expr_types: &'a [HirType],
    pub(crate) call_type_args: &'a HashMap<ExprId, Vec<HirType>>,
    pub(crate) fn_specs: HashMap<(Symbol, Vec<HirType>), HirFn>,
    pub(crate) struct_specs: HashMap<(Symbol, Vec<HirType>), StructDef>,
    pub(crate) type_overrides: HashMap<ExprId, HirType>,
    pub(crate) fn_work_queue: Vec<(Symbol, Vec<HirType>)>,
    pub(crate) fn_queued: HashSet<(Symbol, Vec<HirType>)>,
}
