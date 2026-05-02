use crate::item::{HirItem, StructDef};
use crate::node::{HirExpr, HirFn, HirStmt};
use crate::types::{ExprId, HirType};
use glyim_interner::{Interner, Symbol};
use std::collections::{HashMap, HashSet};

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
    current_type_params: Vec<Symbol>,
}

impl<'a> MonoContext<'a> {
    fn new(
        hir: &'a crate::Hir,
        interner: &'a mut Interner,
        expr_types: &'a [HirType],
        call_type_args: &'a HashMap<ExprId, Vec<HirType>>,
    ) -> Self {
        Self {
            hir,
            interner,
            expr_types,
            call_type_args,
            fn_specs: HashMap::new(),
            struct_specs: HashMap::new(),
            type_overrides: HashMap::new(),
            fn_work_queue: Vec::new(),
            fn_queued: HashSet::new(),
            current_type_params: vec![],
        }
    }

    fn find_fn(&mut self, name: Symbol) -> Option<HirFn> {
        let name_str = self.interner.resolve(name).to_string();

        // Search top-level functions
        for item in &self.hir.items {
            if let HirItem::Fn(f) = item {
                if f.name == name {
                    return Some(f.clone());
                }
            }
        }

        // Search impl methods by exact mangled name
        for item in &self.hir.items {
            if let HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    if m.name == name {
                        return Some(m.clone());
                    }
                }
            }
        }

        // Fallback: search impl methods by demangling (e.g. "push" → "Vec_push")
        if let Some(pos) = name_str.rfind('_') {
            let base_method_name = name_str[pos + 1..].to_string();
            let prefix = name_str[..pos].to_string();
            let prefix_sym = self.interner.intern(&prefix);
            if self.find_struct(prefix_sym).is_some() {
                for item in &self.hir.items {
                    if let HirItem::Impl(imp) = item {
                        if imp.target_name == prefix_sym {
                            for m in &imp.methods {
                                let m_name = self.interner.resolve(m.name).to_string();
                                if m_name == base_method_name
                                    || m_name.ends_with(&format!("_{}", base_method_name))
                                {
                                    return Some(m.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }

    fn find_struct(&self, name: Symbol) -> Option<StructDef> {
        for item in &self.hir.items {
            if let HirItem::Struct(s) = item {
                if s.name == name {
                    return Some(s.clone());
                }
            }
        }
        None
    }

    fn mangle_name(&mut self, base: Symbol, type_args: &[HirType]) -> Symbol {
        let base_str = self.interner.resolve(base).to_string();
        let args_str = type_args
            .iter()
            .map(|t| self.format_type_short(t))
            .collect::<Vec<_>>()
            .join("_");
        self.interner.intern(&format!("{}__{}", base_str, args_str))
    }

    #[tracing::instrument(skip_all)]
    fn collect_and_specialize(&mut self) {
        // Phase A: Use call_type_args from explicit calls AND method calls
        // First, resolve any naked type params (Named(sym) where sym is a type param) to Int
        let resolved_args: HashMap<ExprId, Vec<HirType>> = self
            .call_type_args
            .iter()
            .map(|(id, args)| {
                let resolved: Vec<HirType> = args
                    .iter()
                    .map(|ty| match ty {
                        HirType::Named(sym) => {
                            let name = self.interner.resolve(*sym);
                            if name.len() == 1 && name.chars().next().unwrap().is_uppercase() {
                                HirType::Int
                            } else {
                                ty.clone()
                            }
                        }
                        _ => ty.clone(),
                    })
                    .collect();
                (*id, resolved)
            })
            .collect();

        for (expr_id, type_args) in resolved_args.iter() {
            for item in &self.hir.items {
                match item {
                    HirItem::Fn(f) => {
                        if let Some(callee) = self.find_callee_by_id(&f.body, *expr_id) {
                            self.queue_fn_specialization(callee, type_args.clone());
                        }
                    }
                    HirItem::Impl(imp) => {
                        for m in &imp.methods {
                            if let Some(callee) = self.find_callee_by_id(&m.body, *expr_id) {
                                self.queue_fn_specialization(callee, type_args.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Phase B: Walk all function bodies to discover more generic calls
        for item in &self.hir.items {
            if let HirItem::Fn(f) = item {
                self.current_type_params = f.type_params.clone();
                self.scan_expr_for_generic_calls(&f.body);
                self.scan_expr_for_struct_instantiations(&f.body);
            }
            if let HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    self.current_type_params = m.type_params.clone();
                    self.scan_expr_for_generic_calls(&m.body);
                    self.scan_expr_for_struct_instantiations(&m.body);
                }
            }
        }

        self.current_type_params = vec![];

        // Phase C: Transitive closure
        while let Some((fn_name, type_args)) = self.fn_work_queue.pop() {
            let key = (fn_name, type_args.clone());
            if self.fn_specs.contains_key(&key) {
                continue;
            }
            if let Some(generic_fn) = self.find_fn(fn_name) {
                let specialized = self.specialize_fn(&generic_fn, &type_args);
                self.current_type_params = vec![];
                self.scan_expr_for_generic_calls(&specialized.body);
                self.scan_expr_for_struct_instantiations(&specialized.body);
                self.fn_specs.insert(key.clone(), specialized.clone());
            }
        }
    }

    fn find_callee_by_id(&mut self, expr: &HirExpr, search_id: ExprId) -> Option<Symbol> {
        // Matches both Call and MethodCall nodes
        match expr {
            HirExpr::Call { id, callee, .. } if *id == search_id => Some(*callee),
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                ..
            } if *id == search_id => {
                let receiver_ty = self.expr_types.get(receiver.get_id().as_usize());
                if let Some(HirType::Named(type_name) | HirType::Generic(type_name, _)) =
                    receiver_ty
                {
                    let mangled = format!(
                        "{}_{}",
                        self.interner.resolve(*type_name),
                        self.interner.resolve(*method_name)
                    );
                    Some(self.interner.intern(&mangled))
                } else {
                    None
                }
            }
            HirExpr::Block { stmts, .. } => stmts.iter().find_map(|s| match s {
                HirStmt::Expr(e) => self.find_callee_by_id(e, search_id),
                HirStmt::Let { value, .. }
                | HirStmt::LetPat { value, .. }
                | HirStmt::Assign { value, .. }
                | HirStmt::AssignField { value, .. } => self.find_callee_by_id(value, search_id),
                HirStmt::AssignDeref { target, value, .. } => self
                    .find_callee_by_id(target, search_id)
                    .or_else(|| self.find_callee_by_id(value, search_id)),
            }),
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => self
                .find_callee_by_id(condition, search_id)
                .or_else(|| self.find_callee_by_id(then_branch, search_id))
                .or_else(|| {
                    else_branch
                        .as_ref()
                        .and_then(|e| self.find_callee_by_id(e, search_id))
                }),
            HirExpr::Match {
                scrutinee, arms, ..
            } => self.find_callee_by_id(scrutinee, search_id).or_else(|| {
                arms.iter().find_map(|(_, guard, body)| {
                    guard
                        .as_ref()
                        .and_then(|g| self.find_callee_by_id(g, search_id))
                        .or_else(|| self.find_callee_by_id(body, search_id))
                })
            }),
            HirExpr::Binary { lhs, rhs, .. } => self
                .find_callee_by_id(lhs, search_id)
                .or_else(|| self.find_callee_by_id(rhs, search_id)),
            HirExpr::Unary { operand, .. } => self.find_callee_by_id(operand, search_id),
            HirExpr::Return { value: Some(v), .. } => self.find_callee_by_id(v, search_id),
            HirExpr::Deref { expr, .. } => self.find_callee_by_id(expr, search_id),
            HirExpr::While {
                condition, body, ..
            } => self
                .find_callee_by_id(condition, search_id)
                .or_else(|| self.find_callee_by_id(body, search_id)),
            _ => None,
        }
    }

    fn scan_expr_for_struct_instantiations(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::StructLit {
                id,
                struct_name,
                fields,
                ..
            } => {
                if let Some(struct_def) = self.find_struct(*struct_name) {
                    if !struct_def.type_params.is_empty() {
                        let field_types: Vec<HirType> = fields
                            .iter()
                            .map(|(_, f)| {
                                self.expr_types
                                    .get(f.get_id().as_usize())
                                    .cloned()
                                    .unwrap_or(HirType::Never)
                            })
                            .collect();
                        let mut sub = HashMap::new();
                        for (i, tp) in struct_def.type_params.iter().enumerate() {
                            if let Some(ft) = struct_def.fields.get(i) {
                                if let HirType::Named(param_sym) = &ft.ty {
                                    if let Some(val_ty) = field_types.get(i) {
                                        if *param_sym == *tp && *val_ty != HirType::Never {
                                            sub.insert(*tp, val_ty.clone());
                                        }
                                    }
                                }
                            }
                        }
                        if sub.len() == struct_def.type_params.len() && !sub.is_empty() {
                            let concrete: Vec<HirType> = struct_def
                                .type_params
                                .iter()
                                .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                                .collect();
                            let key = (*struct_name, concrete.clone());
                            if !self.struct_specs.contains_key(&key) {
                                let specialized = self.specialize_struct(&struct_def, &concrete);
                                self.struct_specs.insert(key, specialized);
                            }
                            let mangled = self.mangle_name(*struct_name, &concrete);
                            self.type_overrides.insert(*id, HirType::Named(mangled));
                        }
                    }
                }
                for (_, f) in fields {
                    self.scan_expr_for_struct_instantiations(f);
                }
            }
            HirExpr::Block { stmts, .. } => {
                for s in stmts {
                    match s {
                        HirStmt::Expr(e) => self.scan_expr_for_struct_instantiations(e),
                        HirStmt::Let { value, .. }
                        | HirStmt::LetPat { value, .. }
                        | HirStmt::Assign { value, .. }
                        | HirStmt::AssignField { value, .. } => {
                            self.scan_expr_for_struct_instantiations(value)
                        }
                        HirStmt::AssignDeref { target, value, .. } => {
                            self.scan_expr_for_struct_instantiations(target);
                            self.scan_expr_for_struct_instantiations(value);
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
                self.scan_expr_for_struct_instantiations(condition);
                self.scan_expr_for_struct_instantiations(then_branch);
                if let Some(e) = else_branch {
                    self.scan_expr_for_struct_instantiations(e);
                }
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                self.scan_expr_for_struct_instantiations(scrutinee);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.scan_expr_for_struct_instantiations(g);
                    }
                    self.scan_expr_for_struct_instantiations(body);
                }
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.scan_expr_for_struct_instantiations(lhs);
                self.scan_expr_for_struct_instantiations(rhs);
            }
            HirExpr::Unary { operand, .. }
            | HirExpr::Deref { expr: operand, .. }
            | HirExpr::As { expr: operand, .. } => {
                self.scan_expr_for_struct_instantiations(operand)
            }
            HirExpr::Return { value: Some(v), .. } => self.scan_expr_for_struct_instantiations(v),
            HirExpr::Return { value: None, .. } => {}
            HirExpr::MethodCall { receiver, args, .. } => {
                self.scan_expr_for_struct_instantiations(receiver);
                for a in args {
                    self.scan_expr_for_struct_instantiations(a);
                }
            }
            HirExpr::Call { args, .. } => {
                for a in args {
                    self.scan_expr_for_struct_instantiations(a);
                }
            }
            _ => {}
        }
    }

    fn queue_fn_specialization(&mut self, name: Symbol, args: Vec<HirType>) {
        let key = (name, args);
        if self.fn_specs.contains_key(&key) || self.fn_queued.contains(&key) {
            return;
        }
        self.fn_queued.insert(key.clone());
        self.fn_work_queue.push(key);
    }

    #[tracing::instrument(skip_all)]
    fn scan_expr_for_generic_calls(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::Call { callee, args, .. } => {
                if let Some(fn_def) = self.find_fn(*callee) {
                    if !fn_def.type_params.is_empty() {
                        let arg_types: Vec<HirType> = args
                            .iter()
                            .map(|a| {
                                self.expr_types
                                    .get(a.get_id().as_usize())
                                    .cloned()
                                    .unwrap_or(HirType::Never)
                            })
                            .collect();
                        let mut sub = HashMap::new();
                        for (param_idx, (_, param_ty)) in fn_def.params.iter().enumerate() {
                            if let HirType::Named(param_sym) = param_ty {
                                if fn_def.type_params.contains(param_sym) {
                                    if let Some(at) = arg_types.get(param_idx) {
                                        if *at != HirType::Never {
                                            sub.insert(*param_sym, at.clone());
                                        }
                                    }
                                }
                            }
                        }
                        if sub.len() == fn_def.type_params.len() {
                            let concrete: Vec<HirType> = fn_def
                                .type_params
                                .iter()
                                .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                                .collect();
                            self.queue_fn_specialization(*callee, concrete);
                        }
                    }
                }
                for a in args {
                    self.scan_expr_for_generic_calls(a);
                }
            }
            HirExpr::MethodCall {
                receiver,
                method_name,
                args,
                ..
            } => {
                let receiver_ty = self.expr_types.get(receiver.get_id().as_usize());
                if let Some(HirType::Generic(type_name, type_args)) = receiver_ty {
                    // Resolve any Named(Symbol) type args to concrete types via the impl
                    let _resolved_args: Vec<HirType> = type_args
                        .iter()
                        .map(|ta| {
                            match ta {
                                HirType::Named(sym) => {
                                    // Check if sym is in current_type_params — if so, resolve from the receiver's actual type map
                                    let name_str = self.interner.resolve(*sym).to_string();
                                    if name_str.len() == 1
                                        && name_str.chars().next().unwrap().is_uppercase()
                                    {
                                        // It's a type parameter like T — try to infer from method args
                                        // For now, fallback to the original type from call_type_args
                                        ta.clone()
                                    } else {
                                        ta.clone()
                                    }
                                }
                                _ => ta.clone(),
                            }
                        })
                        .collect();

                    let mangled = format!(
                        "{}_{}",
                        self.interner.resolve(*type_name),
                        self.interner.resolve(*method_name)
                    );
                    let mangled_sym = self.interner.intern(&mangled);
                eprintln!("[mono] MethodCall mangled={} mangled_sym={} fn_map_keys: {:?}", mangled, self.interner.resolve(mangled_sym), fn_map.keys().map(|(s,_)| self.interner.resolve(*s)).collect::<Vec<_>>());

                    // Try to find the method; if it exists with type_params, queue specialization
                    // with concrete args from the receiver type (not the naked type params)
                    let has_impl = self.find_fn(mangled_sym).is_some();
                    let concrete_args: Vec<HirType> = type_args
                        .iter()
                        .map(|ta| {
                            // If ta is a type parameter (single uppercase letter), replace with Int
                            let name_str = self
                                .interner
                                .resolve(match ta {
                                    HirType::Named(s) => *s,
                                    _ => return ta.clone(),
                                })
                                .to_string();
                            if name_str.len() == 1
                                && name_str.chars().next().unwrap().is_uppercase()
                            {
                                HirType::Int
                            } else {
                                ta.clone()
                            }
                        })
                        .collect();

                    if has_impl && !concrete_args.is_empty() {
                        self.queue_fn_specialization(mangled_sym, concrete_args);
                    }
                }
                self.scan_expr_for_generic_calls(receiver);
                for a in args {
                    self.scan_expr_for_generic_calls(a);
                }
            }
            HirExpr::Block { stmts, .. } => {
                for s in stmts {
                    match s {
                        HirStmt::Expr(e) => self.scan_expr_for_generic_calls(e),
                        HirStmt::Let { value, .. }
                        | HirStmt::LetPat { value, .. }
                        | HirStmt::Assign { value, .. }
                        | HirStmt::AssignField { value, .. } => {
                            self.scan_expr_for_generic_calls(value)
                        }
                        HirStmt::AssignDeref { target, value, .. } => {
                            self.scan_expr_for_generic_calls(target);
                            self.scan_expr_for_generic_calls(value);
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
                self.scan_expr_for_generic_calls(condition);
                self.scan_expr_for_generic_calls(then_branch);
                if let Some(e) = else_branch {
                    self.scan_expr_for_generic_calls(e);
                }
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                self.scan_expr_for_generic_calls(scrutinee);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.scan_expr_for_generic_calls(g);
                    }
                    self.scan_expr_for_generic_calls(body);
                }
            }
            HirExpr::While {
                condition, body, ..
            } => {
                self.scan_expr_for_generic_calls(condition);
                self.scan_expr_for_generic_calls(body);
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.scan_expr_for_generic_calls(lhs);
                self.scan_expr_for_generic_calls(rhs);
            }
            HirExpr::Unary { operand, .. }
            | HirExpr::Deref { expr: operand, .. }
            | HirExpr::As { expr: operand, .. } => self.scan_expr_for_generic_calls(operand),
            HirExpr::Return { value: Some(v), .. } => self.scan_expr_for_generic_calls(v),
            HirExpr::Return { value: None, .. } => {}
            _ => {}
        }
    }
    fn format_type_short(&self, ty: &HirType) -> String {
        match ty {
            HirType::Int => "i64".to_string(),
            HirType::Bool => "bool".to_string(),
            HirType::Float => "f64".to_string(),
            HirType::Str => "str".to_string(),
            HirType::Unit => "unit".to_string(),
            HirType::Never => "never".to_string(),
            HirType::Named(s) => self.interner.resolve(*s).to_string(),
            HirType::Generic(s, args) => {
                let inner = args
                    .iter()
                    .map(|a| self.format_type_short(a))
                    .collect::<Vec<_>>()
                    .join("_");
                if inner.is_empty() {
                    self.interner.resolve(*s).to_string()
                } else {
                    format!("{}_{}", self.interner.resolve(*s), inner)
                }
            }
            HirType::Tuple(elems) => format!(
                "tup_{}",
                elems
                    .iter()
                    .map(|e| self.format_type_short(e))
                    .collect::<Vec<_>>()
                    .join("_")
            ),
            HirType::RawPtr(inner) => format!("ptr_{}", self.format_type_short(inner)),
            HirType::Option(inner) => format!("opt_{}", self.format_type_short(inner)),
            HirType::Result(ok, err) => format!(
                "res_{}_{}",
                self.format_type_short(ok),
                self.format_type_short(err)
            ),
            _ => format!("ty{:?}", std::mem::discriminant(ty)),
        }
    }

    #[tracing::instrument(skip_all)]
    fn specialize_fn(&mut self, f: &HirFn, concrete: &[HirType]) -> HirFn {
        let mut sub = HashMap::new();
        for (i, tp) in f.type_params.iter().enumerate() {
            if let Some(ct) = concrete.get(i) {
                sub.insert(*tp, ct.clone());
            }
        }
        // Register any concrete structs used as type arguments so codegen can find them
        for ct in concrete {
            self.ensure_struct_specialized(ct);
        }
        // CRITICAL: collect type overrides before substitution so codegen gets resolved types
        self.collect_type_overrides_for_expr(&f.body, &sub);
        let mut mono = f.clone();
        mono.type_params.clear();
        for (_, pt) in &mut mono.params {
            *pt = crate::types::substitute_type(pt, &sub);
        }
        if let Some(rt) = &mut mono.ret {
            *rt = crate::types::substitute_type(rt, &sub);
        }
        mono.body = self.substitute_expr_types(&mono.body, &sub);
        mono
    }

    /// Ensure a struct type used as a concrete type argument is registered
    fn ensure_struct_specialized(&mut self, ty: &HirType) {
        match ty {
            HirType::Generic(sym, args) => {
                if self.find_struct(*sym).is_some() {
                    let concrete: Vec<HirType> = args.clone();
                    let key = (*sym, concrete.clone());
                    if !self.struct_specs.contains_key(&key) {
                        if let Some(s) = self.find_struct(*sym) {
                            let specialized = self.specialize_struct(&s, &concrete);
                            self.struct_specs.insert(key, specialized);
                        }
                    }
                }
                // Recurse into nested generic args
                for arg in args {
                    self.ensure_struct_specialized(arg);
                }
            }
            _ => {}
        }
    }

    fn collect_type_overrides_for_expr(&mut self, expr: &HirExpr, sub: &HashMap<Symbol, HirType>) {
        let id = expr.get_id();
        if let Some(original_ty) = self.expr_types.get(id.as_usize()) {
            let new_ty = crate::types::substitute_type(original_ty, sub);
            if new_ty != *original_ty {
                self.type_overrides.insert(id, new_ty);
            }
        }
        match expr {
            HirExpr::Block { stmts, .. } => {
                for s in stmts {
                    self.collect_type_overrides_for_stmt(s, sub);
                }
            }
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.collect_type_overrides_for_expr(condition, sub);
                self.collect_type_overrides_for_expr(then_branch, sub);
                if let Some(e) = else_branch {
                    self.collect_type_overrides_for_expr(e, sub);
                }
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                self.collect_type_overrides_for_expr(scrutinee, sub);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.collect_type_overrides_for_expr(g, sub);
                    }
                    self.collect_type_overrides_for_expr(body, sub);
                }
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.collect_type_overrides_for_expr(lhs, sub);
                self.collect_type_overrides_for_expr(rhs, sub);
            }
            HirExpr::Unary { operand, .. }
            | HirExpr::Deref { expr: operand, .. }
            | HirExpr::FieldAccess {
                object: operand, ..
            }
            | HirExpr::As { expr: operand, .. } => {
                self.collect_type_overrides_for_expr(operand, sub)
            }
            HirExpr::Return { value: Some(v), .. } => self.collect_type_overrides_for_expr(v, sub),
            HirExpr::While {
                condition, body, ..
            }
            | HirExpr::ForIn {
                iter: condition,
                body,
                ..
            } => {
                self.collect_type_overrides_for_expr(condition, sub);
                self.collect_type_overrides_for_expr(body, sub);
            }
            HirExpr::MethodCall { receiver, args, .. } => {
                self.collect_type_overrides_for_expr(receiver, sub);
                for a in args {
                    self.collect_type_overrides_for_expr(a, sub);
                }
            }
            HirExpr::Call { args, .. } => {
                for a in args {
                    self.collect_type_overrides_for_expr(a, sub);
                }
            }
            HirExpr::StructLit { fields, .. } => {
                for (_, f) in fields {
                    self.collect_type_overrides_for_expr(f, sub);
                }
            }
            HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => {
                for a in args {
                    self.collect_type_overrides_for_expr(a, sub);
                }
            }
            HirExpr::Println { arg, .. } => self.collect_type_overrides_for_expr(arg, sub),
            HirExpr::Assert {
                condition, message, ..
            } => {
                self.collect_type_overrides_for_expr(condition, sub);
                if let Some(m) = message {
                    self.collect_type_overrides_for_expr(m, sub);
                }
            }
            _ => {}
        }
    }

    fn collect_type_overrides_for_stmt(&mut self, stmt: &HirStmt, sub: &HashMap<Symbol, HirType>) {
        match stmt {
            HirStmt::Let { value, .. }
            | HirStmt::LetPat { value, .. }
            | HirStmt::Assign { value, .. } => self.collect_type_overrides_for_expr(value, sub),
            HirStmt::AssignField { object, value, .. } => {
                self.collect_type_overrides_for_expr(object, sub);
                self.collect_type_overrides_for_expr(value, sub);
            }
            HirStmt::AssignDeref { target, value, .. } => {
                self.collect_type_overrides_for_expr(target, sub);
                self.collect_type_overrides_for_expr(value, sub);
            }
            HirStmt::Expr(e) => self.collect_type_overrides_for_expr(e, sub),
        }
    }

    fn substitute_expr_types(&mut self, expr: &HirExpr, sub: &HashMap<Symbol, HirType>) -> HirExpr {
        match expr {
            HirExpr::SizeOf {
                id,
                target_type,
                span,
            } => HirExpr::SizeOf {
                id: *id,
                target_type: crate::types::substitute_type(target_type, sub),
                span: *span,
            },
            HirExpr::As {
                id,
                expr: inner,
                target_type,
                span,
            } => HirExpr::As {
                id: *id,
                expr: Box::new(self.substitute_expr_types(inner, sub)),
                target_type: crate::types::substitute_type(target_type, sub),
                span: *span,
            },
            HirExpr::Block { id, stmts, span } => HirExpr::Block {
                id: *id,
                stmts: stmts
                    .iter()
                    .map(|s| self.substitute_stmt_types(s, sub))
                    .collect(),
                span: *span,
            },
            HirExpr::Binary {
                id,
                op,
                lhs,
                rhs,
                span,
            } => HirExpr::Binary {
                id: *id,
                op: op.clone(),
                lhs: Box::new(self.substitute_expr_types(lhs, sub)),
                rhs: Box::new(self.substitute_expr_types(rhs, sub)),
                span: *span,
            },
            HirExpr::If {
                id,
                condition,
                then_branch,
                else_branch,
                span,
            } => HirExpr::If {
                id: *id,
                condition: Box::new(self.substitute_expr_types(condition, sub)),
                then_branch: Box::new(self.substitute_expr_types(then_branch, sub)),
                else_branch: else_branch
                    .as_ref()
                    .map(|e| Box::new(self.substitute_expr_types(e, sub))),
                span: *span,
            },
            HirExpr::Match {
                id,
                scrutinee,
                arms,
                span,
            } => HirExpr::Match {
                id: *id,
                scrutinee: Box::new(self.substitute_expr_types(scrutinee, sub)),
                arms: arms
                    .iter()
                    .map(|(pat, guard, body)| {
                        (
                            pat.clone(),
                            guard.as_ref().map(|g| self.substitute_expr_types(g, sub)),
                            self.substitute_expr_types(body, sub),
                        )
                    })
                    .collect(),
                span: *span,
            },
            HirExpr::Call {
                id,
                callee,
                args,
                span,
            } => HirExpr::Call {
                id: *id,
                callee: *callee,
                args: args
                    .iter()
                    .map(|a| self.substitute_expr_types(a, sub))
                    .collect(),
                span: *span,
            },
            HirExpr::Unary {
                id,
                op,
                operand,
                span,
            } => HirExpr::Unary {
                id: *id,
                op: op.clone(),
                operand: Box::new(self.substitute_expr_types(operand, sub)),
                span: *span,
            },
            HirExpr::Return { id, value, span } => HirExpr::Return {
                id: *id,
                value: value
                    .as_ref()
                    .map(|v| Box::new(self.substitute_expr_types(v, sub))),
                span: *span,
            },
            HirExpr::StructLit {
                id,
                struct_name,
                fields,
                span,
            } => HirExpr::StructLit {
                id: *id,
                struct_name: *struct_name,
                fields: fields
                    .iter()
                    .map(|(s, e)| (*s, self.substitute_expr_types(e, sub)))
                    .collect(),
                span: *span,
            },
            HirExpr::EnumVariant {
                id,
                enum_name,
                variant_name,
                args,
                span,
            } => HirExpr::EnumVariant {
                id: *id,
                enum_name: *enum_name,
                variant_name: *variant_name,
                args: args
                    .iter()
                    .map(|a| self.substitute_expr_types(a, sub))
                    .collect(),
                span: *span,
            },
            HirExpr::While {
                id,
                condition,
                body,
                span,
            } => HirExpr::While {
                id: *id,
                condition: Box::new(self.substitute_expr_types(condition, sub)),
                body: Box::new(self.substitute_expr_types(body, sub)),
                span: *span,
            },
            HirExpr::Deref {
                id,
                expr: inner,
                span,
            } => HirExpr::Deref {
                id: *id,
                expr: Box::new(self.substitute_expr_types(inner, sub)),
                span: *span,
            },
            HirExpr::FieldAccess {
                id,
                object,
                field,
                span,
            } => HirExpr::FieldAccess {
                id: *id,
                object: Box::new(self.substitute_expr_types(object, sub)),
                field: *field,
                span: *span,
            },
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                args,
                span,
            } => HirExpr::MethodCall {
                id: *id,
                receiver: Box::new(self.substitute_expr_types(receiver, sub)),
                method_name: *method_name,
                args: args
                    .iter()
                    .map(|a| self.substitute_expr_types(a, sub))
                    .collect(),
                span: *span,
            },
            HirExpr::TupleLit { id, elements, span } => HirExpr::TupleLit {
                id: *id,
                elements: elements
                    .iter()
                    .map(|e| self.substitute_expr_types(e, sub))
                    .collect(),
                span: *span,
            },
            HirExpr::Println { id, arg, span } => HirExpr::Println {
                id: *id,
                arg: Box::new(self.substitute_expr_types(arg, sub)),
                span: *span,
            },
            HirExpr::Assert {
                id,
                condition,
                message,
                span,
            } => HirExpr::Assert {
                id: *id,
                condition: Box::new(self.substitute_expr_types(condition, sub)),
                message: message
                    .as_ref()
                    .map(|m| Box::new(self.substitute_expr_types(m, sub))),
                span: *span,
            },
            _ => expr.clone(),
        }
    }

    fn substitute_stmt_types(&mut self, stmt: &HirStmt, sub: &HashMap<Symbol, HirType>) -> HirStmt {
        match stmt {
            HirStmt::Let {
                name,
                mutable,
                value,
                span,
            } => HirStmt::Let {
                name: *name,
                mutable: *mutable,
                value: self.substitute_expr_types(value, sub),
                span: *span,
            },
            HirStmt::LetPat {
                pattern,
                mutable,
                value,
                span,
                ty,
            } => HirStmt::LetPat {
                pattern: pattern.clone(),
                mutable: *mutable,
                value: self.substitute_expr_types(value, sub),
                ty: ty.clone(),
                span: *span,
            },
            HirStmt::Assign {
                target,
                value,
                span,
            } => HirStmt::Assign {
                target: *target,
                value: self.substitute_expr_types(value, sub),
                span: *span,
            },
            HirStmt::AssignField {
                object,
                field,
                value,
                span,
            } => HirStmt::AssignField {
                object: Box::new(self.substitute_expr_types(object, sub)),
                field: *field,
                value: self.substitute_expr_types(value, sub),
                span: *span,
            },
            HirStmt::AssignDeref {
                target,
                value,
                span,
            } => HirStmt::AssignDeref {
                target: Box::new(self.substitute_expr_types(target, sub)),
                value: self.substitute_expr_types(value, sub),
                span: *span,
            },
            HirStmt::Expr(e) => HirStmt::Expr(self.substitute_expr_types(e, sub)),
        }
    }

    fn specialize_struct(&mut self, s: &StructDef, concrete: &[HirType]) -> StructDef {
        let mut sub = HashMap::new();
        for (i, tp) in s.type_params.iter().enumerate() {
            if let Some(ct) = concrete.get(i) {
                sub.insert(*tp, ct.clone());
            }
        }
        let mut mono = s.clone();
        mono.type_params.clear();
        for field in &mut mono.fields {
            field.ty = crate::types::substitute_type(&field.ty, &sub);
        }
        mono
    }

    #[tracing::instrument(skip_all)]
    fn build_result(mut self) -> MonoResult {
        let mut items = Vec::new();

        // 1. Specialized structs first (clone to avoid borrow conflict)
        let struct_specs: Vec<_> = self
            .struct_specs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        for ((_orig_name, args), s) in &struct_specs {
            let mut mono_s = s.clone();
            mono_s.name = self.mangle_name(s.name, args);
            items.push(HirItem::Struct(mono_s));
        }

        // Build mangling maps for rewriting call sites
        let fn_keys: Vec<(Symbol, Vec<HirType>)> = self.fn_specs.keys().cloned().collect();
        let fn_mangled_names: Vec<((Symbol, Vec<HirType>), Symbol)> = {
            let mut names = Vec::new();
            for (name, args) in &fn_keys {
                let mangled = self.mangle_name(*name, args);
                names.push(((*name, args.clone()), mangled));
            }
            names
        };
        let fn_mangle_map: HashMap<(Symbol, Vec<HirType>), Symbol> =
            fn_mangled_names.into_iter().collect();

        let struct_keys: Vec<(Symbol, Vec<HirType>)> = self.struct_specs.keys().cloned().collect();
        let struct_mangled_names: Vec<(Symbol, Symbol)> = {
            let mut names = Vec::new();
            for (name, args) in &struct_keys {
                let mangled = self.mangle_name(*name, args);
                names.push((*name, mangled));
            }
            names
        };
        let struct_mangle_map: HashMap<Symbol, Symbol> = struct_mangled_names.into_iter().collect();

        // 2. Original items (include ALL functions, not just non-generic)
        let original_items: Vec<crate::item::HirItem> = self
            .hir
            .items
            .iter()
            .filter_map(|item| match item {
                HirItem::Fn(_) => Some(item.clone()),
                HirItem::Struct(_) => Some(item.clone()),
                HirItem::Enum(_) => Some(item.clone()),
                HirItem::Extern(_) => Some(item.clone()),
                HirItem::Impl(imp) => {
                    if !imp.methods.is_empty() {
                        Some(item.clone())
                    } else {
                        None
                    }
                }
            })
            .collect();

        for item in &original_items {
            match item {
                HirItem::Fn(f) => {
                    // Skip all generic functions (they are replaced by specializations)
                    if f.type_params.is_empty() {
                        let rewritten = self.rewrite_fn(f, &fn_mangle_map, &struct_mangle_map);
                        items.push(HirItem::Fn(rewritten));
                    }
                }
                HirItem::Struct(s) => items.push(HirItem::Struct(s.clone())),
                HirItem::Enum(e) => items.push(HirItem::Enum(e.clone())),
                HirItem::Extern(e) => items.push(HirItem::Extern(e.clone())),
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        if m.type_params.is_empty() {
                        let rewritten = self.rewrite_fn(m, &fn_mangle_map, &struct_mangle_map);
                        items.push(HirItem::Fn(rewritten));
                    }
                        }
                }
            }
        }

        // 3. Specialized functions with MANGLED names
        let fn_specs_clone: Vec<_> = self
            .fn_specs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        for ((orig_name, args), f) in &fn_specs_clone {
            let mut mono_f = f.clone();
            mono_f.name = self.mangle_name(*orig_name, args);
            // Rewrite internal MethodCall nodes to use mangled names
            let rewritten = self.rewrite_fn(&mono_f, &fn_mangle_map, &struct_mangle_map);
            items.push(HirItem::Fn(rewritten));
        }

        MonoResult {
            hir: crate::Hir { items },
            type_overrides: self.type_overrides,
        }
    }
    fn rewrite_fn(
        &mut self,
        f: &HirFn,
        fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>,
        struct_map: &HashMap<Symbol, Symbol>,
    ) -> HirFn {
        let mut mono = f.clone();
        mono.body = self.rewrite_expr(&f.body, fn_map, struct_map);
        mono
    }

    #[tracing::instrument(skip_all)]
    fn rewrite_expr(
        &mut self,
        expr: &HirExpr,
        fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>,
        struct_map: &HashMap<Symbol, Symbol>,
    ) -> HirExpr {
        match expr {
            HirExpr::Call {
                id,
                callee,
                args,
                span,
            } => {
                let type_args = self.call_type_args.get(id).cloned().unwrap_or_else(|| {
                    // Fallback: use only the first type_params.len() args
                    // (the rest are value args, not type args)
                    vec![]
                });
                let new_callee = fn_map
                    .get(&(*callee, type_args))
                    .copied()
                    .unwrap_or(*callee);
                HirExpr::Call {
                    id: *id,
                    callee: new_callee,
                    args: args
                        .iter()
                        .map(|a| self.rewrite_expr(a, fn_map, struct_map))
                        .collect(),
                    span: *span,
                }
            }
            HirExpr::StructLit {
                id,
                struct_name,
                fields,
                span,
            } => {
                let new_name = struct_map.get(struct_name).copied().unwrap_or(*struct_name);
                HirExpr::StructLit {
                    id: *id,
                    struct_name: new_name,
                    fields: fields
                        .iter()
                        .map(|(s, e)| (*s, self.rewrite_expr(e, fn_map, struct_map)))
                        .collect(),
                    span: *span,
                }
            }
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                args,
                span,
            } => {
                let rewritten_receiver = Box::new(self.rewrite_expr(receiver, fn_map, struct_map));
                let rewritten_args: Vec<HirExpr> = args
                    .iter()
                    .map(|a| self.rewrite_expr(a, fn_map, struct_map))
                    .collect();
                let receiver_ty = self.expr_types.get(receiver.get_id().as_usize());
                tracing::info!(
                    method = %self.interner.resolve(*method_name),
                    receiver_ty = ?receiver_ty,
                    "Processing MethodCall"
                );
                if let Some(HirType::Named(type_name) | HirType::Generic(type_name, _)) =
                    receiver_ty
                {
                    let mangled = format!(
                        "{}_{}",
                        self.interner.resolve(*type_name),
                        self.interner.resolve(*method_name)
                    );
                    let mangled_sym = self.interner.intern(&mangled);
                eprintln!("[mono] MethodCall mangled={} mangled_sym={} fn_map_keys: {:?}", mangled, self.interner.resolve(mangled_sym), fn_map.keys().map(|(s,_)| self.interner.resolve(*s)).collect::<Vec<_>>());
                    // Try to find a monomorphized version
                    // Collect the receiver's concrete type args to match
                    let receiver_type_args: Vec<HirType> = match receiver_ty {
                        Some(HirType::Generic(_, ref args)) => args.clone(),
                        _ => vec![],
                    };
                    tracing::info!(receiver_type_args = ?receiver_type_args, "Collected receiver type args");
                    if let Some(concrete_key) =
                        fn_map.iter().find_map(|((sym, args), mono_name)| {
                            let matched = *sym == mangled_sym && *args == receiver_type_args;
                            tracing::info!(
                                check_sym = %self.interner.resolve(*sym),
                                check_args = ?args,
                                target_sym = %self.interner.resolve(mangled_sym),
                                target_args = ?receiver_type_args,
                                matched = matched,
                                "Checking fn_map entry"
                            );
                            if matched {
                                Some((args.clone(), *mono_name))
                            } else {
                                None
                            }
                        })
                    {
                        // Wrap the receiver so the Call arm sees a single argument
                        // (the receiver), preventing the fallback from collecting
                        // receiver + method_args as separate type args.
                        let mut all_args = vec![*rewritten_receiver.clone()];
                        all_args.extend(rewritten_args);
<<<<<<< HEAD
                        tracing::info!(mono_name = %self.interner.resolve(concrete_key.1), "Specialization matched, rewriting to Call");
||||||| parent of d514b58 (debug: MethodCall rewrite not firing for let n = self.buckets.len())
=======
                        eprintln!("[mono] Rewrote MethodCall to Call");
>>>>>>> d514b58 (debug: MethodCall rewrite not firing for let n = self.buckets.len())
                        return HirExpr::Call {
                            id: *id,
                            callee: concrete_key.1,
                            args: all_args,
                            span: *span,
                        };
                    }
                }
                tracing::info!("No specialization found for MethodCall, keeping as-is");
                HirExpr::MethodCall {
                    id: *id,
                    receiver: rewritten_receiver,
                    method_name: *method_name,
                    args: rewritten_args,
                    span: *span,
                }
            }
            HirExpr::Block { id, stmts, span } => HirExpr::Block {
                id: *id,
                stmts: stmts
                    .iter()
                    .map(|s| self.rewrite_stmt(s, fn_map, struct_map))
                    .collect(),
                span: *span,
            },
            HirExpr::If {
                id,
                condition,
                then_branch,
                else_branch,
                span,
            } => HirExpr::If {
                id: *id,
                condition: Box::new(self.rewrite_expr(condition, fn_map, struct_map)),
                then_branch: Box::new(self.rewrite_expr(then_branch, fn_map, struct_map)),
                else_branch: else_branch
                    .as_ref()
                    .map(|e| Box::new(self.rewrite_expr(e, fn_map, struct_map))),
                span: *span,
            },
            HirExpr::Match {
                id,
                scrutinee,
                arms,
                span,
            } => {
                let new_arms: Vec<_> = arms
                    .iter()
                    .map(|(pat, guard, body)| {
                        (
                            pat.clone(),
                            guard
                                .as_ref()
                                .map(|g| Box::new(self.rewrite_expr(g, fn_map, struct_map)))
                                .map(|b| *b),
                            self.rewrite_expr(body, fn_map, struct_map),
                        )
                    })
                    .collect();
                HirExpr::Match {
                    id: *id,
                    scrutinee: Box::new(self.rewrite_expr(scrutinee, fn_map, struct_map)),
                    arms: new_arms,
                    span: *span,
                }
            }
            HirExpr::Binary {
                id,
                op,
                lhs,
                rhs,
                span,
            } => HirExpr::Binary {
                id: *id,
                op: op.clone(),
                lhs: Box::new(self.rewrite_expr(lhs, fn_map, struct_map)),
                rhs: Box::new(self.rewrite_expr(rhs, fn_map, struct_map)),
                span: *span,
            },
            HirExpr::Unary {
                id,
                op,
                operand,
                span,
            } => HirExpr::Unary {
                id: *id,
                op: op.clone(),
                operand: Box::new(self.rewrite_expr(operand, fn_map, struct_map)),
                span: *span,
            },
            HirExpr::Return { id, value, span } => HirExpr::Return {
                id: *id,
                value: value
                    .as_ref()
                    .map(|v| Box::new(self.rewrite_expr(v, fn_map, struct_map))),
                span: *span,
            },
            HirExpr::Deref { id, expr, span } => HirExpr::Deref {
                id: *id,
                expr: Box::new(self.rewrite_expr(expr, fn_map, struct_map)),
                span: *span,
            },
            HirExpr::ForIn {
                id,
                pattern,
                iter,
                body,
                span,
            } => HirExpr::ForIn {
                id: *id,
                pattern: pattern.clone(),
                iter: Box::new(self.rewrite_expr(iter, fn_map, struct_map)),
                body: Box::new(self.rewrite_expr(body, fn_map, struct_map)),
                span: *span,
            },
            HirExpr::While {
                id,
                condition,
                body,
                span,
            } => HirExpr::While {
                id: *id,
                condition: Box::new(self.rewrite_expr(condition, fn_map, struct_map)),
                body: Box::new(self.rewrite_expr(body, fn_map, struct_map)),
                span: *span,
            },
            _ => expr.clone(),
        }
    }

    #[tracing::instrument(skip_all)]
    fn rewrite_stmt(
        &mut self,
        stmt: &HirStmt,
        fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>,
        struct_map: &HashMap<Symbol, Symbol>,
    ) -> HirStmt {
        match stmt {
            HirStmt::Let {
                name,
                mutable,
                value,
                span,
            } => HirStmt::Let {
                name: *name,
                mutable: *mutable,
                value: self.rewrite_expr(value, fn_map, struct_map),
                span: *span,
            },
            HirStmt::LetPat {
                pattern,
                mutable,
                value,
                span,
                ty,
            } => HirStmt::LetPat {
                pattern: pattern.clone(),
                mutable: *mutable,
                value: self.rewrite_expr(value, fn_map, struct_map),
                ty: ty.clone(),
                span: *span,
            },
            HirStmt::Assign {
                target,
                value,
                span,
            } => HirStmt::Assign {
                target: *target,
                value: self.rewrite_expr(value, fn_map, struct_map),
                span: *span,
            },
            HirStmt::AssignDeref {
                target,
                value,
                span,
            } => HirStmt::AssignDeref {
                target: Box::new(self.rewrite_expr(target, fn_map, struct_map)),
                value: self.rewrite_expr(value, fn_map, struct_map),
                span: *span,
            },
            HirStmt::AssignField {
                object,
                field,
                value,
                span,
            } => HirStmt::AssignField {
                object: Box::new(self.rewrite_expr(object, fn_map, struct_map)),
                field: *field,
                value: self.rewrite_expr(value, fn_map, struct_map),
                span: *span,
            },
            HirStmt::Expr(e) => HirStmt::Expr(self.rewrite_expr(e, fn_map, struct_map)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::HirItem;
    use glyim_interner::Interner;

    fn lower_source(source: &str) -> (crate::Hir, Interner) {
        let parse_out = glyim_parse::parse(source);
        if !parse_out.errors.is_empty() {
            panic!("parse errors: {:?}", parse_out.errors);
        }
        let mut interner = parse_out.interner;
        (crate::lower(&parse_out.ast, &mut interner), interner)
    }

    #[test]
    fn mono_non_generic_passthrough() {
        let (hir, mut interner) = lower_source("main = () => 42");
        let result = monomorphize(&hir, &mut interner, &[], &HashMap::new());
        assert_eq!(result.hir.items.len(), hir.items.len());
    }

    #[test]
    fn mono_generic_fn_with_call_type_args() {
        let (hir, mut interner) = lower_source("fn id<T>(x: T) -> T { x }\nmain = () => id(42)");
        let main_fn = hir
            .items
            .iter()
            .find(|i| matches!(i, HirItem::Fn(f) if interner.resolve(f.name) == "main"))
            .unwrap();
        let main_fn_body = if let HirItem::Fn(f) = main_fn {
            &f.body
        } else {
            panic!("expected Fn")
        };
        let call_id = find_call_id(main_fn_body, interner.intern("id")).expect("call id");
        let call_type_args = HashMap::from([(call_id, vec![HirType::Int])]);
        let result = monomorphize(&hir, &mut interner, &[], &call_type_args);
        let has_specialized =
            result.hir.items.iter().any(
                |i| matches!(i, HirItem::Fn(f) if interner.resolve(f.name).starts_with("id__")),
            );
        assert!(has_specialized);
    }

    fn find_call_id(expr: &HirExpr, callee: Symbol) -> Option<ExprId> {
        match expr {
            HirExpr::Call { id, callee: c, .. } if *c == callee => Some(*id),
            HirExpr::Block { stmts, .. } => stmts.iter().find_map(|s| match s {
                HirStmt::Expr(e) => find_call_id(e, callee),
                HirStmt::Let { value, .. } => find_call_id(value, callee),
                _ => None,
            }),
            HirExpr::If {
                then_branch,
                else_branch,
                ..
            } => find_call_id(then_branch, callee)
                .or_else(|| else_branch.as_ref().and_then(|e| find_call_id(e, callee))),
            HirExpr::Match { arms, .. } => arms
                .iter()
                .find_map(|(_, _, body)| find_call_id(body, callee)),
            HirExpr::Return { value: Some(v), .. } => find_call_id(v, callee),
            _ => None,
        }
    }
}
