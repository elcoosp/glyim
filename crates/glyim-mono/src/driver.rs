use crate::concretize;
use crate::mangle_table::MangleTable;
use crate::metadata::{TypeMetadata, TypeStructure};
use crate::queue::{ItemKind, WorkItem, WorkItemContext, WorkQueue};
use glyim_diag::Span;
use glyim_hir::HirExpr;
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
    pub specializations: Vec<(Symbol, Symbol, Vec<HirType>)>,
}

pub struct MonoDriver<'a> {
    output_fn_types: HashMap<Symbol, HashMap<ExprId, HirType>>,
    mangle: MangleTable,
    metadata: TypeMetadata,
    queue: WorkQueue,
    interner: &'a mut Interner,
    input_fn_types: &'a HashMap<Symbol, FnTypes>,
    hir: &'a glyim_hir::Hir,
    metrics: MonoMetrics,
    failed_items: Vec<FailedItem>,
    pub(crate) specializations: Vec<(Symbol, Symbol, Vec<HirType>)>,
}

impl<'a> MonoDriver<'a> {
    pub fn new(
        interner: &'a mut Interner,
        input_fn_types: &'a HashMap<Symbol, FnTypes>,
        hir: &'a glyim_hir::Hir,
    ) -> Self {
        Self {
            output_fn_types: HashMap::new(),
            mangle: MangleTable::new(),
            metadata: TypeMetadata::new(),
            queue: WorkQueue::new(),
            interner,
            input_fn_types,
            hir,
            metrics: MonoMetrics::default(),
            failed_items: Vec::new(),
            specializations: Vec::new(),
        }
    }

    pub fn run(mut self) -> MonoResult {
        self.seed_queue();

        while let Some((item, ctx)) = self.queue.pop() {
            match item.kind {
                ItemKind::FnSpecialize => self.process_fn(&item, &ctx, true),
                ItemKind::FnPassthrough => self.process_fn(&item, &ctx, false),
                ItemKind::StructSpecialize => self.process_struct_specialize(&item, &ctx),
                ItemKind::StructPassthrough => self.process_struct_passthrough(&item, &ctx),
                ItemKind::EnumSpecialize => self.process_enum_specialize(&item, &ctx),
                ItemKind::EnumPassthrough => self.process_enum_passthrough(&item, &ctx),
            }
        }

        MonoResult {
            fn_types_map: self.output_fn_types,
            metadata: self.metadata,
            metrics: self.metrics,
            failed_items: self.failed_items,
            specializations: self.specializations,
        }
    }

    fn find_item(&self, sym: Symbol) -> Option<&glyim_hir::HirItem> {
        self.hir.items.iter().find(|i| match i {
            glyim_hir::HirItem::Fn(f) => f.name == sym,
            glyim_hir::HirItem::Struct(s) => s.name == sym,
            glyim_hir::HirItem::Enum(e) => e.name == sym,
            glyim_hir::HirItem::Impl(imp) => imp.methods.iter().any(|m| m.name == sym),
            _ => false,
        })
    }

    fn process_fn(&mut self, item: &WorkItem, ctx: &WorkItemContext, is_specialization: bool) {
        if is_specialization {
            self.metrics.fn_specializations += 1;
        } else {
            self.metrics.fn_passthroughs += 1;
        }

        // Clone everything we need from the HIR before mutating self
        let (fn_body, sub, fn_types_clone) = {
            let fn_def = match self.find_item(item.def_id) {
                Some(glyim_hir::HirItem::Fn(f)) => f,
                Some(glyim_hir::HirItem::Impl(imp)) => {
                    match imp.methods.iter().find(|m| m.name == item.def_id) {
                        Some(m) => m,
                        None => {
                            self.record_failed_item(item, ctx, "Method not found in impl");
                            return;
                        }
                    }
                }
                _ => {
                    self.record_failed_item(item, ctx, "Function definition not found in HIR");
                    return;
                }
            };

            let sub: HashMap<Symbol, HirType> = fn_def
                .type_params
                .iter()
                .zip(item.type_args.iter())
                .map(|(p, a)| (*p, a.clone()))
                .collect();

            let fn_types = match self.input_fn_types.get(&item.def_id) {
                Some(ft) => ft.clone(),
                None => {
                    self.record_failed_item(item, ctx, "Function types not found");
                    return;
                }
            };

            (fn_def.body.clone(), sub, fn_types)
        };

        // Now we have clones, so we can mutate self freely
        let fn_types = match self.input_fn_types.get(&item.def_id) {
            Some(ft) => ft,
            None => return,
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
                    let c2 = c.clone();
                    new_expr_types.insert(expr_id, c);
                    self.queue_type_dependency(&c2);
                }
                Err(e) => {
                    self.record_failed_item_concretize(item, ctx, e);
                    return;
                }
            }
        }

        let mangled_name = if is_specialization {
            match glyim_hir::mangling::mangle_name(self.interner, item.def_id, &item.type_args) {
                Ok(s) => s,
                Err(e) => {
                    self.record_failed_item_mangle(item, ctx, e);
                    return;
                }
            }
        } else {
            item.def_id
        };

        self.output_fn_types.insert(mangled_name, new_expr_types);
        self.scan_and_queue_fn_deps(&fn_body, &sub, &fn_types_clone, ctx);
    }

    fn scan_and_queue_fn_deps(
        &mut self,
        body: &HirExpr,
        sub: &HashMap<Symbol, HirType>,
        fn_types: &FnTypes,
        ctx: &WorkItemContext,
    ) {
        self.scan_expr(body, sub, fn_types, ctx);
    }

    fn scan_expr(
        &mut self,
        expr: &HirExpr,
        sub: &HashMap<Symbol, HirType>,
        fn_types: &FnTypes,
        ctx: &WorkItemContext,
    ) {
        match expr {
            HirExpr::Call {
                id, callee, args, ..
            } => {
                if let Some(call_site_args) = fn_types.call_type_args.get(id) {
                    let callee_name = if let HirExpr::Ident { name, .. } = callee.as_ref() {
                        *name
                    } else {
                        for arg in args {
                            self.scan_expr(arg, sub, fn_types, ctx);
                        }
                        return;
                    };

                    let mut concrete_call_args = Vec::new();
                    for arg in call_site_args {
                        let substituted = glyim_hir::types::substitute_type(arg, sub);
                        match concretize::concretize_and_register(
                            substituted,
                            self.interner,
                            &mut self.mangle,
                            &mut self.metadata,
                            ctx.discovery_span,
                        ) {
                            Ok(c) => {
                                let c2 = c.clone();
                                concrete_call_args.push(c);
                                self.queue_type_dependency(&c2);
                            }
                            Err(_) => continue,
                        }
                    }

                    if let Some(callee_fn_types) = self.input_fn_types.get(&callee_name) {
                        if callee_fn_types.is_generic && !concrete_call_args.is_empty() {
                            if let Ok(mangled) = glyim_hir::mangling::mangle_name(
                                self.interner,
                                callee_name,
                                &concrete_call_args,
                            ) {
                                self.specializations.push((
                                    mangled,
                                    callee_name,
                                    concrete_call_args.clone(),
                                ));
                                self.queue.push(
                                    WorkItem {
                                        kind: ItemKind::FnSpecialize,
                                        def_id: callee_name,
                                        type_args: concrete_call_args,
                                    },
                                    ctx.clone(),
                                    mangled,
                                );
                            }
                        } else if !callee_fn_types.is_generic {
                            self.queue.push(
                                WorkItem {
                                    kind: ItemKind::FnPassthrough,
                                    def_id: callee_name,
                                    type_args: vec![],
                                },
                                ctx.clone(),
                                callee_name,
                            );
                        }
                    }
                }
                for arg in args {
                    self.scan_expr(arg, sub, fn_types, ctx);
                }
            }
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                args,
                ..
            } => {
                if let Some(call_site_args) = fn_types.call_type_args.get(id) {
                    let mut concrete_call_args = Vec::new();
                    for arg in call_site_args {
                        let substituted = glyim_hir::types::substitute_type(arg, sub);
                        match concretize::concretize_and_register(
                            substituted,
                            self.interner,
                            &mut self.mangle,
                            &mut self.metadata,
                            ctx.discovery_span,
                        ) {
                            Ok(c) => {
                                let c2 = c.clone();
                                concrete_call_args.push(c);
                                self.queue_type_dependency(&c2);
                            }
                            Err(_) => continue,
                        }
                    }

                    if let Some(callee_fn_types) = self.input_fn_types.get(method_name) {
                        if callee_fn_types.is_generic && !concrete_call_args.is_empty() {
                            if let Ok(mangled) = glyim_hir::mangling::mangle_name(
                                self.interner,
                                *method_name,
                                &concrete_call_args,
                            ) {
                                self.specializations.push((
                                    mangled,
                                    *method_name,
                                    concrete_call_args.clone(),
                                ));
                                self.queue.push(
                                    WorkItem {
                                        kind: ItemKind::FnSpecialize,
                                        def_id: *method_name,
                                        type_args: concrete_call_args,
                                    },
                                    ctx.clone(),
                                    mangled,
                                );
                            }
                        }
                    }
                }
                self.scan_expr(receiver, sub, fn_types, ctx);
                for arg in args {
                    self.scan_expr(arg, sub, fn_types, ctx);
                }
            }
            HirExpr::Block { stmts, .. } => {
                for stmt in stmts {
                    match stmt {
                        glyim_hir::HirStmt::Let { value, .. }
                        | glyim_hir::HirStmt::LetPat { value, .. }
                        | glyim_hir::HirStmt::Assign { value, .. }
                        | glyim_hir::HirStmt::AssignDeref { value, .. }
                        | glyim_hir::HirStmt::AssignField { value, .. }
                        | glyim_hir::HirStmt::Expr(value) => {
                            self.scan_expr(value, sub, fn_types, ctx);
                        }
                    }
                }
            }
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.scan_expr(condition, sub, fn_types, ctx);
                self.scan_expr(then_branch, sub, fn_types, ctx);
                if let Some(eb) = else_branch {
                    self.scan_expr(eb, sub, fn_types, ctx);
                }
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                self.scan_expr(scrutinee, sub, fn_types, ctx);
                for arm in arms {
                    self.scan_expr(&arm.body, sub, fn_types, ctx);
                }
            }
            HirExpr::While {
                condition, body, ..
            }
            | HirExpr::ForIn {
                iter: condition,
                body,
                ..
            } => {
                self.scan_expr(condition, sub, fn_types, ctx);
                self.scan_expr(body, sub, fn_types, ctx);
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.scan_expr(lhs, sub, fn_types, ctx);
                self.scan_expr(rhs, sub, fn_types, ctx);
            }
            HirExpr::Unary { operand, .. }
            | HirExpr::Deref { expr: operand, .. }
            | HirExpr::As { expr: operand, .. } => {
                self.scan_expr(operand, sub, fn_types, ctx);
            }
            HirExpr::Return { value: Some(v), .. } => {
                self.scan_expr(v, sub, fn_types, ctx);
            }
            HirExpr::StructLit { fields, .. } => {
                for (_, v) in fields {
                    self.scan_expr(v, sub, fn_types, ctx);
                }
            }
            HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => {
                for a in args {
                    self.scan_expr(a, sub, fn_types, ctx);
                }
            }
            _ => {}
        }
    }

    fn queue_type_dependency(&mut self, ty: &HirType) {
        if let HirType::Named(sym) = ty {
            if self.mangle.contains(*sym) {
                return;
            }
            if let Some(TypeStructure::Generic { base, args }) = self.metadata.get(*sym).cloned() {
                if let Some(item) = self.find_item(base) {
                    let kind = match item {
                        glyim_hir::HirItem::Struct(_) => ItemKind::StructSpecialize,
                        glyim_hir::HirItem::Enum(_) => ItemKind::EnumSpecialize,
                        _ => return,
                    };
                    self.queue.push(
                        WorkItem {
                            kind,
                            def_id: base,
                            type_args: args.clone(),
                        },
                        WorkItemContext {
                            discovered_from: None,
                            discovery_span: Span::new(0, 0),
                        },
                        *sym,
                    );
                    self.specializations.push((*sym, base, args));
                }
            }
        }
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
            }
        }
    }

    fn process_struct_specialize(&mut self, item: &WorkItem, ctx: &WorkItemContext) {
        self.metrics.struct_specializations += 1;
        let mangled =
            match glyim_hir::mangling::mangle_name(self.interner, item.def_id, &item.type_args) {
                Ok(s) => s,
                Err(e) => {
                    self.record_failed_item_mangle(item, ctx, e);
                    return;
                }
            };
        self.metadata.record(
            mangled,
            TypeStructure::Generic {
                base: item.def_id,
                args: item.type_args.clone(),
            },
        );
        self.mangle.mark_seen(mangled);
    }

    fn process_struct_passthrough(&mut self, item: &WorkItem, _ctx: &WorkItemContext) {
        self.metrics.struct_passthroughs += 1;
        if !self.mangle.contains(item.def_id) {
            self.metadata
                .record(item.def_id, TypeStructure::Plain { base: item.def_id });
            self.mangle.mark_seen(item.def_id);
        }
    }

    fn process_enum_specialize(&mut self, item: &WorkItem, ctx: &WorkItemContext) {
        self.metrics.enum_specializations += 1;
        let mangled =
            match glyim_hir::mangling::mangle_name(self.interner, item.def_id, &item.type_args) {
                Ok(s) => s,
                Err(e) => {
                    self.record_failed_item_mangle(item, ctx, e);
                    return;
                }
            };
        self.metadata.record(
            mangled,
            TypeStructure::Generic {
                base: item.def_id,
                args: item.type_args.clone(),
            },
        );
        self.mangle.mark_seen(mangled);
    }

    fn process_enum_passthrough(&mut self, item: &WorkItem, _ctx: &WorkItemContext) {
        self.metrics.enum_passthroughs += 1;
        if !self.mangle.contains(item.def_id) {
            self.metadata
                .record(item.def_id, TypeStructure::Plain { base: item.def_id });
            self.mangle.mark_seen(item.def_id);
        }
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
        e: glyim_hir::mangling::ManglingError,
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

pub fn run_on_hir(
    hir: &glyim_hir::Hir,
    interner: &mut Interner,
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
) -> (glyim_hir::Hir, MonoResult) {
    let mut fn_types_map: HashMap<Symbol, FnTypes> = HashMap::new();
    for item in &hir.items {
        if let glyim_hir::HirItem::Fn(f) = item {
            fn_types_map.insert(
                f.name,
                FnTypes {
                    expr_types: HashMap::new(),
                    call_type_args: call_type_args.clone(),
                    sizeof_types: HashMap::new(),
                    is_generic: !f.type_params.is_empty(),
                    type_params: f.type_params.clone(),
                    span: f.span,
                },
            );
        }
        if let glyim_hir::HirItem::Impl(imp) = item {
            for m in &imp.methods {
                let is_generic = !imp.type_params.is_empty() || !m.type_params.is_empty();
                let all_tp: Vec<_> = imp
                    .type_params
                    .iter()
                    .chain(m.type_params.iter())
                    .copied()
                    .collect();
                fn_types_map.insert(
                    m.name,
                    FnTypes {
                        expr_types: HashMap::new(),
                        call_type_args: call_type_args.clone(),
                        sizeof_types: HashMap::new(),
                        is_generic,
                        type_params: all_tp,
                        span: m.span,
                    },
                );
            }
        }
    }

    let driver = MonoDriver::new(interner, &fn_types_map, hir);
    let result = driver.run();

    let mut mono_hir = hir.clone();
    let mut all_specializations: std::collections::HashSet<(Symbol, Vec<HirType>)> =
        std::collections::HashSet::new();

    for item in &hir.items {
        match item {
            glyim_hir::HirItem::Fn(f) if !f.type_params.is_empty() => {
                if let Some(ft) = fn_types_map.get(&f.name) {
                    for type_args in ft.call_type_args.values() {
                        if !type_args.is_empty() {
                            all_specializations.insert((f.name, type_args.clone()));
                        }
                    }
                }
            }
            glyim_hir::HirItem::Impl(imp) => {
                for m in &imp.methods {
                    if !m.type_params.is_empty() {
                        if let Some(ft) = fn_types_map.get(&m.name) {
                            for type_args in ft.call_type_args.values() {
                                if !type_args.is_empty() {
                                    all_specializations.insert((m.name, type_args.clone()));
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for &(_, base_sym, ref type_args) in &result.specializations {
        all_specializations.insert((base_sym, type_args.clone()));
    }

    let mut mangled_names: HashMap<(Symbol, Vec<HirType>), Symbol> = HashMap::new();
    let mut base_to_mangled: HashMap<Symbol, Symbol> = HashMap::new();

    for (base_sym, type_args) in &all_specializations {
        if let Ok(mangled) = glyim_hir::mangling::mangle_name(interner, *base_sym, type_args) {
            mangled_names.insert((*base_sym, type_args.clone()), mangled);
            base_to_mangled.insert(*base_sym, mangled);
        }
    }

    for ((base_sym, type_args), mangled_sym) in &mangled_names {
        if let Some(f) = hir.items.iter().find_map(|item| match item {
            glyim_hir::HirItem::Fn(f) if f.name == *base_sym => Some(f),
            glyim_hir::HirItem::Impl(imp) => imp.methods.iter().find(|m| m.name == *base_sym),
            _ => None,
        }) {
            let mut mono_fn = f.clone();
            mono_fn.name = *mangled_sym;
            mono_fn.type_params.clear();
            let subst: HashMap<_, _> = f
                .type_params
                .iter()
                .zip(type_args.iter())
                .map(|(p, a)| (*p, a.clone()))
                .collect();
            for (_, pt) in &mut mono_fn.params {
                *pt = glyim_hir::types::substitute_type(pt, &subst);
            }
            if let Some(ref mut r) = mono_fn.ret {
                *r = glyim_hir::types::substitute_type(r, &subst);
            }
            rewrite_concrete_body(&mut mono_fn.body, interner, &base_to_mangled);
            mono_hir.items.push(glyim_hir::HirItem::Fn(mono_fn));
        }
    }

    for item in &mut mono_hir.items {
        match item {
            glyim_hir::HirItem::Fn(f) => {
                rewrite_concrete_body(&mut f.body, interner, &base_to_mangled)
            }
            glyim_hir::HirItem::Impl(imp) => {
                for m in &mut imp.methods {
                    rewrite_concrete_body(&mut m.body, interner, &base_to_mangled);
                }
            }
            _ => {}
        }
    }

    (mono_hir, result)
}

fn rewrite_concrete_body(
    expr: &mut HirExpr,
    interner: &mut Interner,
    sub_map: &HashMap<Symbol, Symbol>,
) {
    match expr {
        HirExpr::Call { callee, args, .. } => {
            if let HirExpr::Ident { name, .. } = callee.as_mut() {
                if let Some(&new_name) = sub_map.get(name) {
                    *name = new_name;
                }
            }
            for a in args {
                rewrite_concrete_body(a, interner, sub_map);
            }
        }
        HirExpr::MethodCall {
            receiver,
            method_name,
            args,
            ..
        } => {
            if let Some(&new_name) = sub_map.get(method_name) {
                *method_name = new_name;
            }
            rewrite_concrete_body(receiver, interner, sub_map);
            for a in args {
                rewrite_concrete_body(a, interner, sub_map);
            }
        }
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    glyim_hir::HirStmt::Let { value, .. }
                    | glyim_hir::HirStmt::LetPat { value, .. }
                    | glyim_hir::HirStmt::Assign { value, .. }
                    | glyim_hir::HirStmt::AssignDeref { value, .. }
                    | glyim_hir::HirStmt::AssignField { value, .. }
                    | glyim_hir::HirStmt::Expr(value) => {
                        rewrite_concrete_body(value, interner, sub_map);
                    }
                }
            }
        }
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_concrete_body(condition, interner, sub_map);
            rewrite_concrete_body(then_branch, interner, sub_map);
            if let Some(eb) = else_branch {
                rewrite_concrete_body(eb, interner, sub_map);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            rewrite_concrete_body(scrutinee, interner, sub_map);
            for arm in arms {
                rewrite_concrete_body(&mut arm.body, interner, sub_map);
            }
        }
        HirExpr::While {
            condition, body, ..
        }
        | HirExpr::ForIn {
            iter: condition,
            body,
            ..
        } => {
            rewrite_concrete_body(condition, interner, sub_map);
            rewrite_concrete_body(body, interner, sub_map);
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            rewrite_concrete_body(lhs, interner, sub_map);
            rewrite_concrete_body(rhs, interner, sub_map);
        }
        HirExpr::Unary { operand, .. }
        | HirExpr::Deref { expr: operand, .. }
        | HirExpr::As { expr: operand, .. }
        | HirExpr::Return {
            value: Some(operand),
            ..
        } => {
            rewrite_concrete_body(operand, interner, sub_map);
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                rewrite_concrete_body(v, interner, sub_map);
            }
        }
        HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => {
            for a in args {
                rewrite_concrete_body(a, interner, sub_map);
            }
        }
        _ => {}
    }
}
