use crate::item::StructDef;
use crate::node::HirFn;
use crate::types::{ExprId, HirType};
use glyim_interner::{Interner, Symbol};
use std::collections::{HashMap, HashSet};

pub mod mangle_table;
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

/// Internal state captured during discovery.
struct MonoDiscovery {
    fn_specs: HashMap<(Symbol, Vec<HirType>), HirFn>,
    struct_specs: HashMap<(Symbol, Vec<HirType>), StructDef>,
    type_overrides: HashMap<ExprId, HirType>,
    call_type_args_overrides: HashMap<ExprId, Vec<HirType>>,
    mangle_table: mangle_table::MangleTable,
    interner: Interner,             // updated interner with mangled symbols
}

/// Phase 1: scan and specialize (mutable internment happens here)
#[tracing::instrument(skip_all)]
fn discover_instantiations(
    hir: &crate::Hir,
    interner: &mut Interner,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
) -> MonoDiscovery {
    let mut ctx = MonoContext::new(hir, interner, expr_types, call_type_args);
    ctx.collect_and_specialize();
    MonoDiscovery {
        fn_specs: ctx.fn_specs,
        struct_specs: ctx.struct_specs,
        type_overrides: ctx.type_overrides,
        call_type_args_overrides: ctx.call_type_args_overrides,
        mangle_table: ctx.mangle_table,
        interner: ctx.interner.clone(),
    }
}

/// Phase 2: rewrite the HIR using the captured discovery data.
fn apply_specializations(
    hir: &crate::Hir,
    interner: &mut Interner,        // must be the updated interner
    discovery: MonoDiscovery,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
) -> MonoResult {
    // Sync the external interner with the discovery's interned symbols
    *interner = discovery.interner;

    let mut ctx = MonoContext::new(hir, interner, expr_types, call_type_args);
    ctx.fn_specs = discovery.fn_specs;
    ctx.struct_specs = discovery.struct_specs;
    ctx.type_overrides = discovery.type_overrides;
    ctx.call_type_args_overrides = discovery.call_type_args_overrides;
    ctx.mangle_table = discovery.mangle_table;
    ctx.build_result()
}

/// Public entry point – two‑phase internally, but external interface unchanged.
#[tracing::instrument(skip_all)]
pub fn monomorphize(
    hir: &crate::Hir,
    interner: &mut Interner,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
) -> MonoResult {
    let discovery = discover_instantiations(hir, interner, expr_types, call_type_args);
    apply_specializations(hir, interner, discovery, expr_types, call_type_args)
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
    pub(crate) call_type_args_overrides: HashMap<ExprId, Vec<HirType>>,
    pub(crate) mangle_table: mangle_table::MangleTable,
}
