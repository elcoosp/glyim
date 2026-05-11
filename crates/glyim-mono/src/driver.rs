use crate::concretize;
use crate::mangle_table::MangleTable;
use crate::metadata::TypeMetadata;
use crate::queue::{ItemKind, WorkItem, WorkItemContext, WorkQueue};
use glyim_hir::types::{HirType, ExprId};
use glyim_diag::Span;
use glyim_interner::{Interner, Symbol};
use glyim_typeck::typeck::FnTypes;
use std::collections::HashMap;

#[derive(Debug)]
pub struct FailedItem {
    pub name: Symbol,
    pub type_args: Vec<HirType>,
    pub discovered_from: Option<Symbol>,
    pub error: crate::concretize::ConcretizeError,
}

#[derive(Debug, Default)]
pub struct MonoMetrics {
    pub fn_specializations: usize,
    pub fn_passthroughs: usize,
    pub struct_specializations: usize,
    pub struct_passthroughs: usize,
    pub enum_specializations: usize,
    pub enum_passthroughs: usize,
    pub errors: usize,
}

pub struct MonoResult {
    pub fn_types_map: HashMap<Symbol, HashMap<ExprId, HirType>>,
    pub metadata: TypeMetadata,
    pub metrics: MonoMetrics,
    pub failed_items: Vec<FailedItem>,
}

type DependencyCache = HashMap<Symbol, Vec<(Symbol, Option<ExprId>, bool)>>;

pub struct MonoDriver<'a> {
    output_fn_types: HashMap<Symbol, HashMap<ExprId, HirType>>,
    mangle: MangleTable,
    metadata: TypeMetadata,
    queue: WorkQueue,
    interner: &'a mut Interner,
    input_fn_types: &'a HashMap<Symbol, FnTypes>,
    metrics: MonoMetrics,
    failed_items: Vec<FailedItem>,
    dep_cache: DependencyCache,
}

impl<'a> MonoDriver<'a> {
    pub fn new(
        interner: &'a mut Interner,
        input_fn_types: &'a HashMap<Symbol, FnTypes>,
    ) -> Self {
        Self {
            output_fn_types: HashMap::new(),
            mangle: MangleTable::new(),
            metadata: TypeMetadata::new(),
            queue: WorkQueue::new(),
            interner,
            input_fn_types,
            metrics: MonoMetrics::default(),
            failed_items: Vec::new(),
            dep_cache: DependencyCache::new(),
        }
    }

    pub fn run(mut self) -> MonoResult {
        self.seed_queue();

        while let Some((item, ctx)) = self.queue.pop() {
            match item.kind {
                ItemKind::FnSpecialize => self.process_fn_specialize(&item, &ctx),
                ItemKind::FnPassthrough => self.process_fn_passthrough(&item, &ctx),
                ItemKind::StructSpecialize | ItemKind::StructPassthrough => {
                    self.process_struct_specialize(&item, &ctx);
                }
                ItemKind::EnumSpecialize | ItemKind::EnumPassthrough => {
                    self.process_enum_specialize(&item, &ctx);
                }
            }
        }

        MonoResult {
            fn_types_map: self.output_fn_types,
            metadata: self.metadata,
            metrics: self.metrics,
            failed_items: self.failed_items,
        }
    }

    fn seed_queue(&mut self) {
        for (&fn_name, fn_types) in self.input_fn_types {
            if !fn_types.is_generic {
                self.queue.push(
                    WorkItem { kind: ItemKind::FnPassthrough, def_id: fn_name, type_args: vec![] },
                    WorkItemContext { discovered_from: None, discovery_span: Span::new(0, 0) },
                    fn_name,
                );
            }
        }
    }

    fn process_fn_specialize(&mut self, item: &WorkItem, ctx: &WorkItemContext) {
        self.metrics.fn_specializations += 1;

        let fn_types = match self.input_fn_types.get(&item.def_id) {
            Some(ft) => ft,
            None => {
                self.record_failed_item(item, ctx, "Original generic fn types missing");
                return;
            }
        };

        let mut new_expr_types = HashMap::new();
        for (&expr_id, ty) in &fn_types.expr_types {
            match concretize::concretize_and_register(
                ty.clone(),
                self.interner,
                &mut self.mangle,
                &mut self.metadata,
                ctx.discovery_span,
            ) {
                Ok(c) => { new_expr_types.insert(expr_id, c); }
                Err(e) => {
                    self.metrics.errors += 1;
                    self.failed_items.push(FailedItem {
                        name: item.def_id,
                        type_args: item.type_args.clone(),
                        discovered_from: ctx.discovered_from,
                        error: e,
                    });
                    return;
                }
            }
        }

        let mangled_name = match crate::mangling::mangle_name(
            self.interner,
            item.def_id,
            &item.type_args,
        ) {
            Ok(s) => s,
            Err(e) => {
                self.record_failed_item_mangle(item, ctx, e);
                return;
            }
        };

        self.output_fn_types.insert(mangled_name, new_expr_types);
    }

    fn process_fn_passthrough(&mut self, item: &WorkItem, ctx: &WorkItemContext) {
        self.metrics.fn_passthroughs += 1;

        if let Some(fn_types) = self.input_fn_types.get(&item.def_id) {
            let mut new_expr_types = HashMap::new();
            for (&id, ty) in &fn_types.expr_types {
                match concretize::concretize_and_register(
                    ty.clone(),
                    self.interner,
                    &mut self.mangle,
                    &mut self.metadata,
                    ctx.discovery_span,
                ) {
                    Ok(c) => { new_expr_types.insert(id, c); }
                    Err(e) => {
                        self.record_failed_item_concretize(item, ctx, e);
                        return;
                    }
                }
            }
            self.output_fn_types.insert(item.def_id, new_expr_types);
        }
    }

    fn process_struct_specialize(&mut self, item: &WorkItem, ctx: &WorkItemContext) {
        match item.kind {
            ItemKind::StructPassthrough => self.metrics.struct_passthroughs += 1,
            _ => self.metrics.struct_specializations += 1,
        }

        let _mangled = match crate::mangling::mangle_name(
            self.interner,
            item.def_id,
            &item.type_args,
        ) {
            Ok(s) => s,
            Err(e) => {
                self.record_failed_item_mangle(item, ctx, e);
                return;
            }
        };

        self.metadata.record(
            item.def_id,
            crate::metadata::TypeStructure::Generic {
                base: item.def_id,
                args: item.type_args.clone(),
            },
        );
    }

    fn process_enum_specialize(&mut self, item: &WorkItem, ctx: &WorkItemContext) {
        match item.kind {
            ItemKind::EnumPassthrough => self.metrics.enum_passthroughs += 1,
            _ => self.metrics.enum_specializations += 1,
        }

        let _mangled = match crate::mangling::mangle_name(
            self.interner,
            item.def_id,
            &item.type_args,
        ) {
            Ok(s) => s,
            Err(e) => {
                self.record_failed_item_mangle(item, ctx, e);
                return;
            }
        };

        self.metadata.record(
            item.def_id,
            crate::metadata::TypeStructure::Generic {
                base: item.def_id,
                args: item.type_args.clone(),
            },
        );
    }

    fn record_failed_item(&mut self, item: &WorkItem, ctx: &WorkItemContext, reason: &str) {
        self.metrics.errors += 1;
        self.failed_items.push(FailedItem {
            name: item.def_id,
            type_args: item.type_args.clone(),
            discovered_from: ctx.discovered_from,
            error: crate::concretize::ConcretizeError {
                kind: crate::concretize::ConcretizeErrorKind::StructuralFailure,
                ty: Box::new(HirType::Error),
                detail: reason.to_string(),
                span: ctx.discovery_span,
            },
        });
    }

    fn record_failed_item_mangle(&mut self, item: &WorkItem, ctx: &WorkItemContext, e: crate::mangling::ManglingError) {
        self.metrics.errors += 1;
        self.failed_items.push(FailedItem {
            name: item.def_id,
            type_args: item.type_args.clone(),
            discovered_from: ctx.discovered_from,
            error: crate::concretize::ConcretizeError {
                kind: crate::concretize::ConcretizeErrorKind::ManglingFailed,
                ty: Box::new(HirType::Error),
                detail: format!("{:?}", e),
                span: ctx.discovery_span,
            },
        });
    }

    fn record_failed_item_concretize(&mut self, item: &WorkItem, ctx: &WorkItemContext, e: crate::concretize::ConcretizeError) {
        self.metrics.errors += 1;
        self.failed_items.push(FailedItem {
            name: item.def_id,
            type_args: item.type_args.clone(),
            discovered_from: ctx.discovered_from,
            error: e,
        });
    }
}
