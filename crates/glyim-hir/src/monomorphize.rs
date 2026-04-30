use std::collections::{HashMap, HashSet};
use crate::item::{HirImplDef, HirItem, StructDef};
use crate::node::{HirExpr, HirStmt, HirFn};
use crate::types::{ExprId, HirType, HirPattern};
use glyim_interner::{Interner, Symbol};

pub struct MonoResult {
    pub hir: crate::Hir,
    pub type_overrides: HashMap<ExprId, HirType>,
}

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
    current_type_params: Vec<Symbol>,  // type params of the function being scanned
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

    fn find_fn(&self, name: Symbol) -> Option<HirFn> {
        // Search top-level functions
        for item in &self.hir.items {
            if let HirItem::Fn(f) = item {
                if f.name == name {
                    return Some(f.clone());
                }
            }
        }
        // Search impl methods
        for item in &self.hir.items {
            if let HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    if m.name == name {
                        return Some(m.clone());
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
        let mangled = format!("{}__{}", base_str, args_str);
        println!("[mono mangle] base='{}' type_args={:?} -> '{}'", base_str, type_args.iter().map(|t| format!("{:?}", t)).collect::<Vec<_>>(), mangled);
        self.interner.intern(&mangled)
    }

    fn collect_and_specialize(&mut self) {
        // Seed work queue from call_type_args (provided by type checker)
        for (expr_id, type_args) in self.call_type_args.iter() {
            for item in &self.hir.items {
                match item {
                    HirItem::Fn(f) => {
                        if let Some(callee) = self.find_call_callee_by_id(&f.body, *expr_id) {
                            self.queue_fn_specialization(callee, type_args.clone());
                        }
                    }
                    HirItem::Impl(imp) => {
                        for m in &imp.methods {
                            if let Some(callee) = self.find_call_callee_by_id(&m.body, *expr_id) {
                                self.queue_fn_specialization(callee, type_args.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        // Also scan all function bodies to discover generic calls
        // not yet in call_type_args (backward compatibility fallback).
        // For non‑generic functions, also scan struct instantiations immediately.
        for item in &self.hir.items {
            if let HirItem::Fn(f) = item {
                self.current_type_params = f.type_params.clone();
                self.scan_expr_for_generic_calls(&f.body);
                if f.type_params.is_empty() {
                    self.scan_expr_for_struct_instantiations(&f.body);
                }
            }
            if let HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    self.current_type_params = m.type_params.clone();
                    self.scan_expr_for_generic_calls(&m.body);
                }
            }
        }
        self.current_type_params = vec![];

        // Struct scanning done in work-queue processing only

        // Process work queue
        while let Some((fn_name, type_args)) = self.fn_work_queue.pop() {
            let key = (fn_name, type_args.clone());
            if self.fn_specs.contains_key(&key) {
                continue;
            }
            if let Some(generic_fn) = self.find_fn(fn_name) {
                let specialized = self.specialize_fn(&generic_fn, &type_args);
                self.current_type_params = vec![];  // specialized body has no type params
                self.scan_expr_for_generic_calls(&specialized.body);
                self.scan_expr_for_struct_instantiations(&specialized.body);
                self.fn_specs.insert(key.clone(), specialized.clone());
            }
        }
    }

    fn find_call_callee_by_id(&self, expr: &HirExpr, search_id: ExprId) -> Option<Symbol> {
        match expr {
            HirExpr::Call { id, callee, .. } if *id == search_id => Some(*callee),
            HirExpr::Block { stmts, .. } => stmts.iter().find_map(|s| {
                match s {
                    HirStmt::Expr(e) => self.find_call_callee_by_id(e, search_id),
                    HirStmt::Let { value, .. } | HirStmt::LetPat { value, .. } | HirStmt::Assign { value, .. } => {
                        self.find_call_callee_by_id(value, search_id)
                    }
                    HirStmt::AssignDeref { target, value, .. } => {
                        self.find_call_callee_by_id(target, search_id)
                            .or_else(|| self.find_call_callee_by_id(value, search_id))
                    }
                }
            }),
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.find_call_callee_by_id(condition, search_id)
                    .or_else(|| self.find_call_callee_by_id(then_branch, search_id))
                    .or_else(|| else_branch.as_ref().and_then(|e| self.find_call_callee_by_id(e, search_id)))
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                self.find_call_callee_by_id(scrutinee, search_id)
                    .or_else(|| arms.iter().find_map(|(_, guard, body)| {
                        guard.as_ref().and_then(|g| self.find_call_callee_by_id(g, search_id))
                            .or_else(|| self.find_call_callee_by_id(body, search_id))
                    }))
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.find_call_callee_by_id(lhs, search_id)
                    .or_else(|| self.find_call_callee_by_id(rhs, search_id))
            }
            HirExpr::Unary { operand, .. } => self.find_call_callee_by_id(operand, search_id),
            HirExpr::Return { value: Some(v), .. } => self.find_call_callee_by_id(v, search_id),
            HirExpr::Deref { expr, .. } => self.find_call_callee_by_id(expr, search_id),
            HirExpr::MethodCall { args, .. } => {
                for a in args {
                    if let Some(sym) = self.find_call_callee_by_id(a, search_id) {
                        return Some(sym);
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn scan_expr_for_struct_instantiations(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::StructLit { id, struct_name, fields, .. } => {
                println!("[mono scan_struct] StructLit id={:?} name='{}'", id, self.interner.resolve(*struct_name));
                if let Some(struct_def) = self.find_struct(*struct_name) {
                    println!("[mono scan_struct] found struct_def with {} type_params", struct_def.type_params.len());
                    if !struct_def.type_params.is_empty() {
                        let field_types: Vec<HirType> = fields
                            .iter()
                            .map(|(_, f)| self.expr_types.get(f.get_id().as_usize()).cloned().unwrap_or(HirType::Never))
                            .collect();
                        println!("[mono scan_struct] field_types: {:?}", field_types.iter().map(|t| format!("{:?}", t)).collect::<Vec<_>>());
                        let mut sub = HashMap::new();
                        for (i, tp) in struct_def.type_params.iter().enumerate() {
                            if let Some(ft) = struct_def.fields.get(i) {
                                println!("[mono scan_struct]   field[{}] ty={:?}", i, ft.ty);
                                if let HirType::Named(param_sym) = &ft.ty {
                                    if let Some(val_ty) = field_types.get(i) {
                                        println!("[mono scan_struct]     val_ty={:?}, param_sym={:?}, tp={:?}, match={}, not_never={}", val_ty, self.interner.resolve(*param_sym), self.interner.resolve(*tp), *param_sym == *tp, *val_ty != HirType::Never);
                                        if *param_sym == *tp && *val_ty != HirType::Never {
                                            sub.insert(*tp, val_ty.clone());
                                        }
                                    }
                                }
                            }
                        }
                        println!("[mono scan_struct] sub len={}, type_params len={}", sub.len(), struct_def.type_params.len());
                        if sub.len() == struct_def.type_params.len() {
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
                            println!("[mono scan_struct] CREATING specialization: mangled='{}'", self.interner.resolve(mangled));
                            self.type_overrides.insert(*id, HirType::Named(mangled));
                        }
                    }
                }
                for (_, f) in fields { self.scan_expr_for_struct_instantiations(f); }
            }
            HirExpr::Block { stmts, .. } => {
                for s in stmts {
                    match s {
                        HirStmt::Expr(e) => self.scan_expr_for_struct_instantiations(e),
                        HirStmt::Let { value, .. } | HirStmt::LetPat { value, .. } | HirStmt::Assign { value, .. } => self.scan_expr_for_struct_instantiations(value),
                        HirStmt::AssignDeref { target, value, .. } => {
                            self.scan_expr_for_struct_instantiations(target);
                            self.scan_expr_for_struct_instantiations(value);
                        }
                    }
                }
            }
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.scan_expr_for_struct_instantiations(condition);
                self.scan_expr_for_struct_instantiations(then_branch);
                if let Some(e) = else_branch { self.scan_expr_for_struct_instantiations(e); }
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                self.scan_expr_for_struct_instantiations(scrutinee);
                for (_, guard, body) in arms {
                    if let Some(g) = guard { self.scan_expr_for_struct_instantiations(g); }
                    self.scan_expr_for_struct_instantiations(body);
                }
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.scan_expr_for_struct_instantiations(lhs);
                self.scan_expr_for_struct_instantiations(rhs);
            }
            HirExpr::Unary { operand, .. } | HirExpr::Deref { expr: operand, .. } => self.scan_expr_for_struct_instantiations(operand),
            HirExpr::Return { value, .. } => if let Some(v) = value { self.scan_expr_for_struct_instantiations(v); }
            HirExpr::MethodCall { args, .. } => for a in args { self.scan_expr_for_struct_instantiations(a); }
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

    fn scan_expr_for_generic_calls(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::Call { callee, args, .. } => {
                if let Some(fn_def) = self.find_fn(*callee) {
                    if !fn_def.type_params.is_empty() {
                        let arg_types: Vec<HirType> = args
                            .iter()
                            .map(|a| self.expr_types.get(a.get_id().as_usize()).cloned().unwrap_or(HirType::Never))
                            .collect();
                        // Map type params to concrete types by matching param positions
                        // where the declared type equals the type param symbol.
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
                for a in args { self.scan_expr_for_generic_calls(a); }
            }
            HirExpr::StructLit { fields, .. } => for (_, f) in fields { self.scan_expr_for_generic_calls(f); },
            HirExpr::Block { stmts, .. } => {
                for s in stmts {
                    match s {
                        HirStmt::Expr(e) => self.scan_expr_for_generic_calls(e),
                        HirStmt::Let { value, .. } | HirStmt::LetPat { value, .. } | HirStmt::Assign { value, .. } => self.scan_expr_for_generic_calls(value),
                        HirStmt::AssignDeref { target, value, .. } => {
                            self.scan_expr_for_generic_calls(target);
                            self.scan_expr_for_generic_calls(value);
                        }
                    }
                }
            }
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.scan_expr_for_generic_calls(condition);
                self.scan_expr_for_generic_calls(then_branch);
                if let Some(e) = else_branch { self.scan_expr_for_generic_calls(e); }
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                self.scan_expr_for_generic_calls(scrutinee);
                for (_, guard, body) in arms {
                    if let Some(g) = guard { self.scan_expr_for_generic_calls(g); }
                    self.scan_expr_for_generic_calls(body);
                }
            }
            HirExpr::Binary { lhs, rhs, .. } => { self.scan_expr_for_generic_calls(lhs); self.scan_expr_for_generic_calls(rhs); }
            HirExpr::Unary { operand, .. } | HirExpr::Deref { expr: operand, .. } => self.scan_expr_for_generic_calls(operand),
            HirExpr::Return { value, .. } => if let Some(v) = value { self.scan_expr_for_generic_calls(v); }
            HirExpr::MethodCall { args, .. } => for a in args { self.scan_expr_for_generic_calls(a); }
            _ => {}
        }
    }

    
    fn format_type_short(&self, ty: &HirType) -> String {
        match ty {
            HirType::Int    => "i64".to_string(),
            HirType::Bool   => "bool".to_string(),
            HirType::Float  => "f64".to_string(),
            HirType::Str    => "str".to_string(),
            HirType::Unit   => "unit".to_string(),
            HirType::Never  => "never".to_string(),
            HirType::Named(s) => self.interner.resolve(*s).to_string(),
            HirType::Generic(s, args) => {
                let inner = args.iter().map(|a| self.format_type_short(a)).collect::<Vec<_>>().join("_");
                if inner.is_empty() { self.interner.resolve(*s).to_string() } else { format!("{}_{}", self.interner.resolve(*s), inner) }
            }
            HirType::Tuple(elems) => {
                format!("tup_{}", elems.iter().map(|e| self.format_type_short(e)).collect::<Vec<_>>().join("_"))
            }
            HirType::RawPtr(inner) => format!("ptr_{}", self.format_type_short(inner)),
            HirType::Option(inner) => format!("opt_{}", self.format_type_short(inner)),
            HirType::Result(ok, err) => format!("res_{}_{}", self.format_type_short(ok), self.format_type_short(err)),
            _ => format!("ty{:?}", std::mem::discriminant(ty)),
        }
    }

    fn specialize_fn(&mut self, f: &HirFn, concrete: &[HirType]) -> HirFn {
        let mut sub = HashMap::new();
        for (i, tp) in f.type_params.iter().enumerate() {
            if let Some(ct) = concrete.get(i) {
                sub.insert(*tp, ct.clone());
            }
        }
        let mut mono = f.clone();
        mono.type_params.clear();
        for (_, pt) in &mut mono.params {
            *pt = crate::types::substitute_type(pt, &sub);
        }
        if let Some(rt) = &mut mono.ret {
            *rt = crate::types::substitute_type(rt, &sub);
        }
        mono
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

    fn build_result(mut self) -> MonoResult {
        // Precompute mangle maps once, before any mutable borrow
        eprintln!("[mono] fn_specs={}, struct_specs={}", self.fn_specs.len(), self.struct_specs.len());
        let fn_keys: Vec<(Symbol, Vec<HirType>)> = self.fn_specs.keys().cloned().collect();
        let struct_keys: Vec<(Symbol, Vec<HirType>)> = self.struct_specs.keys().cloned().collect();

        let fn_mangle_map: HashMap<(Symbol, Vec<HirType>), Symbol> = fn_keys
            .iter()
            .map(|(name, args)| ((*name, args.clone()), self.mangle_name(*name, args)))
            .collect();

        let struct_mangle_map: HashMap<Symbol, Symbol> = struct_keys
            .iter()
            .map(|(name, args)| (*name, self.mangle_name(*name, args)))
            .collect();

        let mut items = Vec::new();

        // 1. Add specialized structs FIRST so codegen registers them before functions
        {
            let struct_entries: Vec<_> = self.struct_specs.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            eprintln!("[mono] pushing {} specialized structs first", struct_entries.len());
            for ((_orig_name, args), mut s) in struct_entries {
                s.name = self.mangle_name(s.name, &args);
                eprintln!("[mono]   struct '{}'", self.interner.resolve(s.name));
                items.push(HirItem::Struct(s));
            }
        }

        // 2. Rewrite non-generic items (functions, impls, etc.)
        for item in &self.hir.items {
            match item {
                HirItem::Fn(f) if f.type_params.is_empty() => {
                    items.push(HirItem::Fn(self.rewrite_fn(f, &fn_mangle_map, &struct_mangle_map)));
                }
                HirItem::Struct(s) if s.type_params.is_empty() => {
                    items.push(HirItem::Struct(s.clone()));
                }
                HirItem::Enum(e) if e.type_params.is_empty() => {
                    items.push(HirItem::Enum(e.clone()));
                }
                HirItem::Extern(e) => {
                    items.push(HirItem::Extern(e.clone()));
                }
                HirItem::Impl(imp) if imp.type_params.is_empty() => {
                    let rewritten = self.rewrite_impl(imp, &fn_mangle_map, &struct_mangle_map);
                    for method in &rewritten.methods {
                        items.push(HirItem::Fn(method.clone()));
                    }
                }
                _ => {}
            }
        }

        // Add specialized functions with rewritten bodies (collect first to avoid borrow conflicts)
        {
            let fn_entries: Vec<_> = self.fn_specs.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for ((_orig_name, args), mut f) in fn_entries {
                f.name = self.mangle_name(f.name, &args);
                f.body = self.rewrite_expr(&f.body, &fn_mangle_map, &struct_mangle_map);
                items.push(HirItem::Fn(f));
            }
        }

        // Build final type_overrides
        let mut final_type_overrides = HashMap::new();
        for (expr_id, ty) in &self.type_overrides {
            if let HirType::Named(orig) = ty {
                if let Some(&mangled) = struct_mangle_map.get(orig) {
                    final_type_overrides.insert(*expr_id, HirType::Named(mangled));
                } else {
                    final_type_overrides.insert(*expr_id, ty.clone());
                }
            }
        }
        for (idx, ty) in self.expr_types.iter().enumerate() {
            if let HirType::Generic(orig, _) = ty {
                if let Some(&mangled) = struct_mangle_map.get(orig) {
                    final_type_overrides.insert(ExprId::new(idx as u32), HirType::Named(mangled));
                }
            }
        }
        for item in &items {
            match item {
                HirItem::Fn(f) => eprintln!("  Fn {:?}", self.interner.resolve(f.name)),
                HirItem::Struct(s) => eprintln!("  Struct {:?} with {} fields", self.interner.resolve(s.name), s.fields.len()),
                _ => eprintln!("  {:?}", std::mem::discriminant(item)),
            }
        }
        MonoResult {
            hir: crate::Hir { items },
            type_overrides: final_type_overrides,
        }
    }

    fn rewrite_fn(&mut self, f: &HirFn, fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>, struct_map: &HashMap<Symbol, Symbol>) -> HirFn {
        let mut mono = f.clone();
        mono.body = self.rewrite_expr(&f.body, fn_map, struct_map);
        mono
    }

    fn rewrite_impl(&mut self, imp: &HirImplDef, fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>, struct_map: &HashMap<Symbol, Symbol>) -> HirImplDef {
        let mut mono = imp.clone();
        for method in &mut mono.methods {
            method.body = self.rewrite_expr(&method.body, fn_map, struct_map);
        }
        mono
    }

    fn rewrite_expr(&mut self, expr: &HirExpr, fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>, struct_map: &HashMap<Symbol, Symbol>) -> HirExpr {
        match expr {
            HirExpr::Call { id, callee, args, span } => {
                // Look up with concrete type args (from call_type_args or inferred from arg types)
                let type_args = self.call_type_args.get(id).cloned().unwrap_or_else(|| {
                    args.iter()
                        .map(|a| self.expr_types.get(a.get_id().as_usize()).cloned().unwrap_or(HirType::Int))
                        .collect()
                });
                let key = (*callee, type_args);
                let new_callee = fn_map.get(&key).copied().unwrap_or(*callee);
                HirExpr::Call {
                    id: *id,
                    callee: new_callee,
                    args: args.iter().map(|a| self.rewrite_expr(a, fn_map, struct_map)).collect(),
                    span: *span,
                }
            }
            HirExpr::StructLit { id, struct_name, fields, span } => {
                let new_name = struct_map.get(struct_name).copied().unwrap_or(*struct_name);
                HirExpr::StructLit {
                    id: *id,
                    struct_name: new_name,
                    fields: fields.iter().map(|(s, e)| (*s, self.rewrite_expr(e, fn_map, struct_map))).collect(),
                    span: *span,
                }
            }
            HirExpr::Block { id, stmts, span } => {
                HirExpr::Block {
                    id: *id,
                    stmts: stmts.iter().map(|s| self.rewrite_stmt(s, fn_map, struct_map)).collect(),
                    span: *span,
                }
            }
            HirExpr::If { id, condition, then_branch, else_branch, span } => {
                HirExpr::If {
                    id: *id,
                    condition: Box::new(self.rewrite_expr(condition, fn_map, struct_map)),
                    then_branch: Box::new(self.rewrite_expr(then_branch, fn_map, struct_map)),
                    else_branch: else_branch.as_ref().map(|e| Box::new(self.rewrite_expr(e, fn_map, struct_map))),
                    span: *span,
                }
            }
            HirExpr::Match { id, scrutinee, arms, span } => {
                let new_arms: Vec<(HirPattern, Option<HirExpr>, HirExpr)> = arms
                    .iter()
                    .map(|(pat, guard, body)| {
                        let new_guard = guard.as_ref()
                            .map(|g| Box::new(self.rewrite_expr(g, fn_map, struct_map)))
                            .map(|b| *b);
                        (pat.clone(), new_guard, self.rewrite_expr(body, fn_map, struct_map))
                    })
                    .collect();
                HirExpr::Match {
                    id: *id,
                    scrutinee: Box::new(self.rewrite_expr(scrutinee, fn_map, struct_map)),
                    arms: new_arms,
                    span: *span,
                }
            }
            HirExpr::Binary { id, op, lhs, rhs, span } => {
                HirExpr::Binary {
                    id: *id, op: op.clone(),
                    lhs: Box::new(self.rewrite_expr(lhs, fn_map, struct_map)),
                    rhs: Box::new(self.rewrite_expr(rhs, fn_map, struct_map)),
                    span: *span,
                }
            }
            HirExpr::Unary { id, op, operand, span } => {
                HirExpr::Unary {
                    id: *id, op: op.clone(),
                    operand: Box::new(self.rewrite_expr(operand, fn_map, struct_map)),
                    span: *span,
                }
            }
            HirExpr::Return { id, value, span } => {
                HirExpr::Return {
                    id: *id,
                    value: value.as_ref().map(|v| Box::new(self.rewrite_expr(v, fn_map, struct_map))),
                    span: *span,
                }
            }
            HirExpr::Deref { id, expr, span } => {
                HirExpr::Deref {
                    id: *id,
                    expr: Box::new(self.rewrite_expr(expr, fn_map, struct_map)),
                    span: *span,
                }
            }
            HirExpr::ForIn { id, pattern, iter, body, span } => {
                HirExpr::ForIn {
                    id: *id,
                    pattern: pattern.clone(),
                    iter: Box::new(self.rewrite_expr(iter, fn_map, struct_map)),
                    body: Box::new(self.rewrite_expr(body, fn_map, struct_map)),
                    span: *span,
                }
            }
            HirExpr::MethodCall { id, receiver, method_name, args, span } => {
                HirExpr::MethodCall {
                    id: *id,
                    receiver: Box::new(self.rewrite_expr(receiver, fn_map, struct_map)),
                    method_name: *method_name,
                    args: args.iter().map(|a| self.rewrite_expr(a, fn_map, struct_map)).collect(),
                    span: *span,
                }
            }
            _ => expr.clone(),
        }
    }

    fn rewrite_stmt(&mut self, stmt: &HirStmt, fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>, struct_map: &HashMap<Symbol, Symbol>) -> HirStmt {
        match stmt {
            HirStmt::Let { name, mutable, value, span } => {
                HirStmt::Let { name: *name, mutable: *mutable, value: self.rewrite_expr(value, fn_map, struct_map), span: *span }
            }
            HirStmt::LetPat { pattern, mutable, value, span } => {
                HirStmt::LetPat { pattern: pattern.clone(), mutable: *mutable, value: self.rewrite_expr(value, fn_map, struct_map), span: *span }
            }
            HirStmt::Assign { target, value, span } => {
                HirStmt::Assign { target: *target, value: self.rewrite_expr(value, fn_map, struct_map), span: *span }
            }
            HirStmt::AssignDeref { target, value, span } => {
                HirStmt::AssignDeref {
                    target: Box::new(self.rewrite_expr(target, fn_map, struct_map)),
                    value: self.rewrite_expr(value, fn_map, struct_map),
                    span: *span,
                }
            }
            HirStmt::Expr(e) => HirStmt::Expr(self.rewrite_expr(e, fn_map, struct_map)),
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::HirItem;
    use crate::node::HirExpr;
    use glyim_interner::Interner;

    fn lower_source(source: &str) -> (crate::Hir, Interner) {
        let parse_out = glyim_parse::parse(source);
        if !parse_out.errors.is_empty() {
            panic!("parse errors: {:?}", parse_out.errors);
        }
        let mut interner = parse_out.interner;
        let hir = crate::lower(&parse_out.ast, &mut interner);
        (hir, interner)
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
        let main_fn = hir.items.iter().find(|i| matches!(i, HirItem::Fn(f) if interner.resolve(f.name) == "main")).unwrap();
        let main_fn_body = if let HirItem::Fn(f) = main_fn { &f.body } else { panic!("expected Fn") };
        let call_id = find_call_id(main_fn_body, interner.intern("id")).expect("call id");
        let call_type_args = HashMap::from([(call_id, vec![HirType::Int])]);
        let result = monomorphize(&hir, &mut interner, &[], &call_type_args);
        let has_specialized = result.hir.items.iter().any(|i| {
            matches!(i, HirItem::Fn(f) if interner.resolve(f.name).starts_with("id__"))
        });
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
            HirExpr::If { then_branch, else_branch, .. } => find_call_id(then_branch, callee)
                .or_else(|| else_branch.as_ref().and_then(|e| find_call_id(e, callee))),
            HirExpr::Match { arms, .. } => arms.iter().find_map(|(_, _, body)| find_call_id(body, callee)),
            HirExpr::Return { value: Some(v), .. } => find_call_id(v, callee),
            _ => None,
        }
    }
}
