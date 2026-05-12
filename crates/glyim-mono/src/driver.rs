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
    pub specializations: Vec<(Symbol, Symbol, Vec<HirType>)>,
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
    /// Each entry: (mangled_name, base_name, concrete_type_args)
    pub(crate) specializations: Vec<(Symbol, Symbol, Vec<HirType>)>,
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
            specializations: Vec::new(),
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
            specializations: self.specializations,
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

        // ── Build monomorphized HIR using the complete set of discovered specializations ──
        let mut mono_hir = hir.clone();

        // 1. Gather all specialisations: those from direct call‑site analysis AND those
        //    discovered by the MonoDriver (transitive closure).
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
        // Add every specialization the driver discovered.
        for &(_, base_sym, ref type_args) in &result.specializations {
            all_specializations.insert((base_sym, type_args.clone()));
        }

        // 2. Build the full mapping: (base_name, type_args) → mangled_symbol
        let mut mangled_names: std::collections::HashMap<(Symbol, Vec<HirType>), Symbol> =
            std::collections::HashMap::new();
        // Also build a simpler map base_name → mangled_symbol for rewrite_concrete_body.
        let mut base_to_mangled: std::collections::HashMap<Symbol, Symbol> =
            std::collections::HashMap::new();

        for (base_sym, type_args) in &all_specializations {
            if let Ok(mangled) = glyim_hir::mangling::mangle_name(interner, *base_sym, type_args) {
                mangled_names.insert((*base_sym, type_args.clone()), mangled);
                base_to_mangled.insert(*base_sym, mangled);
            }
        }

        // 3. For every specialization, clone the generic function, substitute types,
        //    rewrite internal calls, and add to HIR.
        for ((base_sym, type_args), &mangled_sym) in &mangled_names {
            if let Some(f) = hir.items.iter().find_map(|item| match item {
                glyim_hir::HirItem::Fn(f) if f.name == *base_sym => Some(f),
                glyim_hir::HirItem::Impl(imp) => imp.methods.iter().find(|m| m.name == *base_sym),
                _ => None,
            }) {
                let mut mono_fn = f.clone();
                mono_fn.name = mangled_sym;
                mono_fn.type_params.clear();

                let subst: std::collections::HashMap<_, _> = f
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

                // Rewrite the body so that internal calls also use concrete names.
                rewrite_concrete_body(&mut mono_fn.body, interner, &base_to_mangled);
                mono_hir.items.push(glyim_hir::HirItem::Fn(mono_fn));
            }
        }

        // 4. Rewrite all call sites in EVERY item (original and new) so that any generic call
        //    is replaced by its concrete mangled form.  This catches calls that were not
        //    covered by the rewrite above (e.g. in main or in other non‑specialised items).
        for item in &mut mono_hir.items {
            match item {
                glyim_hir::HirItem::Fn(f) => {
                    rewrite_concrete_body(&mut f.body, interner, &base_to_mangled);
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
            match glyim_hir::mangling::mangle_name(self.interner, item.def_id, &item.type_args) {
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
            match glyim_hir::mangling::mangle_name(self.interner, item.def_id, &item.type_args) {
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
            match glyim_hir::mangling::mangle_name(self.interner, item.def_id, &item.type_args) {
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

/// Walk the entire HIR and rewrite Calls whose callee names are generic functions
/// for which we have recorded concrete type arguments in `call_type_args`.
/// The callee's name is replaced with the mangled (concrete) name.
fn rewrite_call_sites(
    hir: &mut glyim_hir::Hir,
    call_type_args: &std::collections::HashMap<
        glyim_hir::types::ExprId,
        Vec<glyim_hir::types::HirType>,
    >,
    interner: &mut glyim_interner::Interner,
) {
    for item in &mut hir.items {
        match item {
            glyim_hir::HirItem::Fn(f) => rewrite_expr(&mut f.body, call_type_args, interner),
            glyim_hir::HirItem::Impl(imp) => {
                for m in &mut imp.methods {
                    rewrite_expr(&mut m.body, call_type_args, interner);
                }
            }
            _ => {}
        }
    }
}

fn rewrite_expr(
    expr: &mut glyim_hir::HirExpr,
    call_type_args: &std::collections::HashMap<
        glyim_hir::types::ExprId,
        Vec<glyim_hir::types::HirType>,
    >,
    interner: &mut glyim_interner::Interner,
) {
    match expr {
        glyim_hir::HirExpr::Call {
            id,
            callee,
            args,
            ..
        } => {
            if let glyim_hir::HirExpr::Ident { name, .. } = callee.as_mut() {
                let name_str = interner.resolve(*name);
                // Only mangle if the name is not already mangled (doesn't contain __)
                if !name_str.contains("__") {
                    if let Some(type_args) = call_type_args.get(id) {
                        if !type_args.is_empty() {
                            if let Ok(mangled) = glyim_hir::mangling::mangle_name(interner, *name, type_args) {
                                *name = mangled;
                            }
                        }
                    }
                }
            }
            for a in args {
                rewrite_expr(a, call_type_args, interner);
            }
        }
        glyim_hir::HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    glyim_hir::HirStmt::Let { value, .. }
                    | glyim_hir::HirStmt::LetPat { value, .. }
                    | glyim_hir::HirStmt::Assign { value, .. }
                    | glyim_hir::HirStmt::AssignDeref { value, .. }
                    | glyim_hir::HirStmt::AssignField { value, .. } => {
                        rewrite_expr(value, call_type_args, interner);
                    }
                    glyim_hir::HirStmt::Expr(e) => rewrite_expr(e, call_type_args, interner),
                }
            }
        }
        glyim_hir::HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_expr(condition, call_type_args, interner);
            rewrite_expr(then_branch, call_type_args, interner);
            if let Some(eb) = else_branch {
                rewrite_expr(eb, call_type_args, interner);
            }
        }
        glyim_hir::HirExpr::Match {
            scrutinee, arms, ..
        } => {
            rewrite_expr(scrutinee, call_type_args, interner);
            for arm in arms {
                rewrite_expr(&mut arm.body, call_type_args, interner);
            }
        }
        glyim_hir::HirExpr::While {
            condition, body, ..
        }
        | glyim_hir::HirExpr::ForIn {
            iter: condition,
            body,
            ..
        } => {
            rewrite_expr(condition, call_type_args, interner);
            rewrite_expr(body, call_type_args, interner);
        }
        glyim_hir::HirExpr::Binary { lhs, rhs, .. } => {
            rewrite_expr(lhs, call_type_args, interner);
            rewrite_expr(rhs, call_type_args, interner);
        }
        glyim_hir::HirExpr::Unary { operand, .. }
        | glyim_hir::HirExpr::Deref { expr: operand, .. }
        | glyim_hir::HirExpr::As { expr: operand, .. }
        | glyim_hir::HirExpr::Return { value: Some(operand), .. } => {
            rewrite_expr(operand, call_type_args, interner);
        }
        glyim_hir::HirExpr::MethodCall {
            receiver,
            method_name,
            args,
            id,
            ..
        } => {
            // Method calls should have been desugared by now, but handle defensively.
            // Only mangle if the method name is not already mangled (doesn't contain __)
            let method_str = interner.resolve(*method_name);
            if !method_str.contains("__") {
                if let Some(type_args) = call_type_args.get(id) {
                    if !type_args.is_empty() {
                        if let Ok(mangled) = glyim_hir::mangling::mangle_name(interner, *method_name, type_args) {
                            *method_name = mangled;
                        }
                    }
                }
            }
            rewrite_expr(receiver, call_type_args, interner);
            for a in args {
                rewrite_expr(a, call_type_args, interner);
            }
        }
        glyim_hir::HirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                rewrite_expr(v, call_type_args, interner);
            }
        }
        glyim_hir::HirExpr::EnumVariant { args, .. }
        | glyim_hir::HirExpr::TupleLit { elements: args, .. } => {
            for a in args {
                rewrite_expr(a, call_type_args, interner);
            }
        }
        _ => {}
    }
}

/// Recursively rewrite callee names in an expression to their concrete mangled versions.
fn rewrite_concrete_body(
    expr: &mut glyim_hir::HirExpr,
    interner: &mut glyim_interner::Interner,
    sub_map: &std::collections::HashMap<glyim_interner::Symbol, glyim_interner::Symbol>,
) {
    match expr {
        glyim_hir::HirExpr::Call {
            callee, args, ..
        } => {
            if let glyim_hir::HirExpr::Ident { name, .. } = callee.as_mut() {
                if let Some(&new_name) = sub_map.get(name) {
                    *name = new_name;
                } else {
                    // Try to match as a method name suffix (internal method calls)
                    let callee_name = interner.resolve(*name);
                    for (&base, &mangled) in sub_map.iter() {
                        let base_name = interner.resolve(base);
                        if base_name.ends_with(&format!("_{}", callee_name)) {
                            *name = mangled;
                            break;
                        }
                    }
                }
            }
            for a in args {
                rewrite_concrete_body(a, interner, sub_map);
            }
        }
        glyim_hir::HirExpr::MethodCall {
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
        glyim_hir::HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    glyim_hir::HirStmt::Let { value, .. }
                    | glyim_hir::HirStmt::LetPat { value, .. }
                    | glyim_hir::HirStmt::Assign { value, .. }
                    | glyim_hir::HirStmt::AssignDeref { value, .. }
                    | glyim_hir::HirStmt::AssignField { value, .. } => {
                        rewrite_concrete_body(value, interner, sub_map);
                    }
                    glyim_hir::HirStmt::Expr(e) => rewrite_concrete_body(e, interner, sub_map),
                }
            }
        }
        glyim_hir::HirExpr::If {
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
        glyim_hir::HirExpr::Match {
            scrutinee, arms, ..
        } => {
            rewrite_concrete_body(scrutinee, interner, sub_map);
            for arm in arms {
                rewrite_concrete_body(&mut arm.body, interner, sub_map);
            }
        }
        glyim_hir::HirExpr::While {
            condition, body, ..
        }
        | glyim_hir::HirExpr::ForIn {
            iter: condition,
            body,
            ..
        } => {
            rewrite_concrete_body(condition, interner, sub_map);
            rewrite_concrete_body(body, interner, sub_map);
        }
        glyim_hir::HirExpr::Binary { lhs, rhs, .. } => {
            rewrite_concrete_body(lhs, interner, sub_map);
            rewrite_concrete_body(rhs, interner, sub_map);
        }
        glyim_hir::HirExpr::Unary { operand, .. }
        | glyim_hir::HirExpr::Deref { expr: operand, .. }
        | glyim_hir::HirExpr::As { expr: operand, .. }
        | glyim_hir::HirExpr::Return { value: Some(operand), .. } => {
            rewrite_concrete_body(operand, interner, sub_map);
        }
        glyim_hir::HirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                rewrite_concrete_body(v, interner, sub_map);
            }
        }
        glyim_hir::HirExpr::EnumVariant { args, .. }
        | glyim_hir::HirExpr::TupleLit { elements: args, .. } => {
            for a in args {
                rewrite_concrete_body(a, interner, sub_map);
            }
        }
        _ => {}
    }
}
