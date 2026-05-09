use crate::item::{EnumDef, StructDef};
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

pub struct MonoResult {
    pub hir: crate::Hir,
    pub type_overrides: HashMap<ExprId, HirType>,
}

// ── internal discovery / application ──

struct MonoDiscovery {
    fn_specs: HashMap<(Symbol, Vec<HirType>), HirFn>,
    struct_specs: HashMap<(Symbol, Vec<HirType>), StructDef>,
    enum_specs: HashMap<(Symbol, Vec<HirType>), EnumDef>,
    type_overrides: HashMap<ExprId, HirType>,
    mangle_table: mangle_table::MangleTable,
    interner: Interner,
}

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
        enum_specs: ctx.enum_specs,
        type_overrides: ctx.type_overrides,
        mangle_table: ctx.mangle_table,
        interner: ctx.interner.clone(),
    }
}

fn apply_specializations(
    hir: &crate::Hir,
    interner: &mut Interner,
    discovery: MonoDiscovery,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
) -> MonoResult {
    *interner = discovery.interner;

    let mut ctx = MonoContext::new(hir, interner, expr_types, call_type_args);
    ctx.fn_specs = discovery.fn_specs;
    ctx.struct_specs = discovery.struct_specs;
    ctx.enum_specs = discovery.enum_specs;
    ctx.type_overrides = discovery.type_overrides;
    ctx.mangle_table = discovery.mangle_table;
    ctx.build_result()
}

pub fn monomorphize(
    hir: &crate::Hir,
    interner: &mut Interner,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
) -> MonoResult {
    let discovery = discover_instantiations(hir, interner, expr_types, call_type_args);
    apply_specializations(hir, interner, discovery, expr_types, call_type_args)
}

// ── context struct (no overrides field) ──

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
    pub(crate) mangle_table: mangle_table::MangleTable,
    pub(crate) enum_specs: HashMap<(Symbol, Vec<HirType>), EnumDef>,
    pub(crate) type_work_queue: Vec<(Symbol, Vec<HirType>)>,
    pub(crate) type_queued: HashSet<(Symbol, Vec<HirType>)>,
    pub(crate) method_map: HashMap<(Symbol, Symbol), HirFn>,
}
