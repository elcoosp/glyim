use crate::concretize;
use crate::mangle_table::MangleTable;
use crate::metadata::TypeMetadata;
use crate::queue::{ItemKind, WorkItem, WorkItemContext, WorkQueue};
use glyim_diag::Span;
use glyim_hir::types::{ExprId, HirType};
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
    pub fn new(interner: &'a mut Interner, input_fn_types: &'a HashMap<Symbol, FnTypes>) -> Self {
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

    /// Run monomorphization on an HIR and return a new HIR with specialized functions added.
    /// This is a free function (not a method) to avoid lifetime issues with &mut Interner.
    pub fn run_on_hir(
        hir: &glyim_hir::Hir,
        interner: &mut Interner,
        call_type_args: &std::collections::HashMap<
            glyim_hir::types::ExprId,
            Vec<glyim_hir::types::HirType>,
        >,
    ) -> (glyim_hir::Hir, MonoResult) {
        // Build fn_types_map
        let mut fn_types_map: std::collections::HashMap<Symbol, glyim_typeck::typeck::FnTypes> =
            std::collections::HashMap::new();
        for item in &hir.items {
            if let glyim_hir::HirItem::Fn(f) = item {
                fn_types_map.insert(
                    f.name,
                    glyim_typeck::typeck::FnTypes {
                        expr_types: std::collections::HashMap::new(),
                        call_type_args: call_type_args.clone(),
                        sizeof_types: std::collections::HashMap::new(),
                        is_generic: !f.type_params.is_empty(),
                        type_params: f.type_params.clone(),
                        span: f.span,
                    },
                );
            }
            if let glyim_hir::HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    fn_types_map.insert(
                        m.name,
                        glyim_typeck::typeck::FnTypes {
                            expr_types: std::collections::HashMap::new(),
                            call_type_args: call_type_args.clone(),
                            sizeof_types: std::collections::HashMap::new(),
                            is_generic: !m.type_params.is_empty(),
                            type_params: m.type_params.clone(),
                            span: m.span,
                        },
                    );
                }
            }
        }

        // Run the driver (this takes &mut interner)
        let driver = MonoDriver::new(interner, &fn_types_map);
        let result = driver.run();
        // driver dropped here - interner borrow released

        // Now build the monomorphized HIR
        let mut mono_hir = hir.clone();
        for item in &hir.items {
            if let glyim_hir::HirItem::Fn(f) = item {
                if !f.type_params.is_empty() {
                    if let Some(ft) = fn_types_map.get(&f.name) {
                        for type_args in ft.call_type_args.values() {
                            if !type_args.is_empty() {
                                if let Ok(mangled) =
                                    crate::mangling::mangle_name(interner, f.name, type_args)
                                {
                                    let mut mono_fn = f.clone();
                                    mono_fn.name = mangled;
                                    mono_fn.type_params.clear();
                                    let sub: std::collections::HashMap<_, _> = f
                                        .type_params
                                        .iter()
                                        .zip(type_args.iter())
                                        .map(|(p, a)| (*p, a.clone()))
                                        .collect();
                                    for (_, pt) in &mut mono_fn.params {
                                        *pt = glyim_hir::types::substitute_type(pt, &sub);
                                    }
                                    if let Some(ref mut r) = mono_fn.ret {
                                        *r = glyim_hir::types::substitute_type(r, &sub);
                                    }
                                    mono_hir.items.push(glyim_hir::HirItem::Fn(mono_fn));
                                }
                            }
                        }
                    }
                }
            }
            if let glyim_hir::HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    if !m.type_params.is_empty() {
                        if let Some(ft) = fn_types_map.get(&m.name) {
                            for type_args in ft.call_type_args.values() {
                                if !type_args.is_empty() {
                                    if let Ok(mangled) =
                                        crate::mangling::mangle_name(interner, m.name, type_args)
                                    {
                                        let mut mono_fn = m.clone();
                                        mono_fn.name = mangled;
                                        mono_fn.type_params.clear();
                                        let sub: std::collections::HashMap<_, _> = m
                                            .type_params
                                            .iter()
                                            .zip(type_args.iter())
                                            .map(|(p, a)| (*p, a.clone()))
                                            .collect();
                                        for (_, pt) in &mut mono_fn.params {
                                            *pt = glyim_hir::types::substitute_type(pt, &sub);
                                        }
                                        if let Some(ref mut r) = mono_fn.ret {
                                            *r = glyim_hir::types::substitute_type(r, &sub);
                                        }
                                        mono_hir.items.push(glyim_hir::HirItem::Fn(mono_fn));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        (mono_hir, result)
    }

    fn seed_queue(&mut self) {
        for (&fn_name, fn_types) in self.input_fn_types {
            if !fn_types.is_generic {
                self.queue.push(
                    WorkItem {
                        kind: ItemKind::FnPassthrough,
                        def_id: fn_name,
                        type_args: vec![],
                    },
                    WorkItemContext {
                        discovered_from: None,
                        discovery_span: Span::new(0, 0),
                    },
                    fn_name,
                );
            } else {
                // Check if this generic function/method is called with concrete type args
                for (_, type_args) in &fn_types.call_type_args {
                    if !type_args.is_empty() {
                        self.queue.push(
                            WorkItem {
                                kind: ItemKind::FnSpecialize,
                                def_id: fn_name,
                                type_args: type_args.clone(),
                            },
                            WorkItemContext {
                                discovered_from: Some(fn_name),
                                discovery_span: Span::new(0, 0),
                            },
                            fn_name,
                        );
                        break; // one specialization per call type args set is enough
                    }
                }
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
                Ok(c) => {
                    new_expr_types.insert(expr_id, c);
                }
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

        let mangled_name =
            match crate::mangling::mangle_name(self.interner, item.def_id, &item.type_args) {
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
                    Ok(c) => {
                        new_expr_types.insert(id, c);
                    }
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

        let _mangled =
            match crate::mangling::mangle_name(self.interner, item.def_id, &item.type_args) {
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

        let _mangled =
            match crate::mangling::mangle_name(self.interner, item.def_id, &item.type_args) {
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

    fn record_failed_item_mangle(
        &mut self,
        item: &WorkItem,
        ctx: &WorkItemContext,
        e: crate::mangling::ManglingError,
    ) {
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

    fn record_failed_item_concretize(
        &mut self,
        item: &WorkItem,
        ctx: &WorkItemContext,
        e: crate::concretize::ConcretizeError,
    ) {
        self.metrics.errors += 1;
        self.failed_items.push(FailedItem {
            name: item.def_id,
            type_args: item.type_args.clone(),
            discovered_from: ctx.discovered_from,
            error: e,
        });
    }
}
