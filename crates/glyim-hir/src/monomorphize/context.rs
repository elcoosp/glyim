use super::*;
use crate::MatchArm;
use crate::item::{EnumDef, HirItem, StructDef};
use crate::node::{HirExpr, HirFn, HirStmt};
use crate::types::HirType;
use glyim_interner::Symbol;
use std::collections::HashMap;

impl<'a> MonoContext<'a> {
    pub(crate) fn new(
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
            mangle_table: mangle_table::MangleTable::new(),
            enum_specs: HashMap::new(),
            type_work_queue: Vec::new(),
            type_queued: HashSet::new(),
            method_map: HashMap::new(),
        }
    }

    pub(crate) fn get_expr_type(&self, id: ExprId) -> Option<HirType> {
        self.type_overrides
            .get(&id)
            .cloned()
            .or_else(|| self.expr_types.get(id.as_usize()).cloned())
    }

    pub(crate) fn find_fn(&mut self, name: Symbol) -> Option<HirFn> {
        let name_str = self.interner.resolve(name).to_string();
        if let Some(pos) = name_str.find("__") {
            let base = self.interner.intern(&name_str[..pos]);
            return self.find_fn(base);
        }
        for item in &self.hir.items {
            if let HirItem::Fn(f) = item
                && f.name == name
            {
                return Some(f.clone());
            }
        }
        for item in &self.hir.items {
            if let HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    if m.name == name {
                        return Some(m.clone());
                    }
                }
            }
        }
        if let Some(uscore) = name_str.rfind('_') {
            let type_name = &name_str[..uscore];
            let method_name = &name_str[uscore + 1..];
            if let Some(type_sym) = self.interner.resolve_symbol(type_name) {
                let method_sym = self.interner.intern(method_name);
                if let Some(method) = self.method_map.get(&(type_sym, method_sym)) {
                    return Some(method.clone());
                }
            }
        }
        None
    }

    pub(crate) fn find_struct(&self, name: Symbol) -> Option<StructDef> {
        for item in &self.hir.items {
            if let HirItem::Struct(s) = item
                && s.name == name
            {
                return Some(s.clone());
            }
        }
        None
    }

    pub(crate) fn find_enum(&self, name: Symbol) -> Option<EnumDef> {
        for item in &self.hir.items {
            if let HirItem::Enum(e) = item
                && e.name == name
            {
                return Some(e.clone());
            }
        }
        None
    }

    pub(crate) fn mangle_name(&mut self, base: Symbol, args: &[HirType]) -> Symbol {
        self.mangle_table.mangle(base, args, self.interner)
    }

    pub(crate) fn has_unresolved_type_param(&self, ty: &HirType) -> bool {
        match ty {
            HirType::Named(sym) => {
                let s = self.interner.resolve(*sym);
                s.len() == 1 && s.chars().next().is_some_and(|c| c.is_uppercase())
            }
            HirType::Generic(_, args) => args.iter().any(|a| self.has_unresolved_type_param(a)),
            HirType::RawPtr(inner) => self.has_unresolved_type_param(inner),
            HirType::Option(inner) => self.has_unresolved_type_param(inner),
            HirType::Result(ok, err) => {
                self.has_unresolved_type_param(ok) || self.has_unresolved_type_param(err)
            }
            HirType::Tuple(elems) => elems.iter().any(|e| self.has_unresolved_type_param(e)),
            _ => false,
        }
    }

    pub(crate) fn substitute_type_args(
        &self,
        args: &[HirType],
        sub: &HashMap<Symbol, HirType>,
    ) -> Vec<HirType> {
        if sub.is_empty() {
            args.to_vec()
        } else {
            args.iter()
                .map(|ty| crate::types::substitute_type(ty, sub))
                .collect()
        }
    }

    pub(crate) fn init_method_map(&mut self) {
        let mut entries: Vec<(Symbol, Symbol, HirFn)> = Vec::new();
        for item in &self.hir.items {
            if let HirItem::Impl(imp) = item {
                let type_sym = imp.target_name;
                let type_name = self.interner.resolve(type_sym).to_string();
                for method in &imp.methods {
                    let mangled = self.interner.resolve(method.name).to_string();
                    if let Some(pos) = mangled.rfind('_') {
                        let type_part = &mangled[..pos];
                        if type_part == type_name {
                            let method_part = &mangled[pos + 1..];
                            let method_sym = self.interner.intern(method_part);
                            entries.push((type_sym, method_sym, method.clone()));
                        }
                    }
                }
            }
        }
        for (type_sym, method_sym, fn_def) in entries {
            self.method_map.insert((type_sym, method_sym), fn_def);
        }
    }

    pub(crate) fn specialize_fn(&mut self, f: &HirFn, concrete: &[HirType]) -> HirFn {
        let mut sub = HashMap::new();

        // Map the function's own type parameters
        eprintln!(
            "[specialize_fn debug] f.name={:?} f.type_params={:?} concrete={:?}",
            f.name, f.type_params, concrete
        );
        for (i, tp) in f.type_params.iter().enumerate() {
            if let Some(ct) = concrete.get(i) {
                sub.insert(*tp, ct.clone());
            }
        }

        // Map type params inferred from self parameter's generic type args
        if !f.params.is_empty() {
            if let (_, HirType::Generic(_, param_args)) = &f.params[0] {
                for (i, formal) in param_args.iter().enumerate() {
                    if let HirType::Named(fname) = formal
                        && let Some(ct) = concrete.get(i)
                    {
                        sub.insert(*fname, ct.clone());
                    }
                }
            }
        }

        // Also map the struct's own type params (may differ from impl's type params)
        if !f.params.is_empty() {
            eprintln!(
                "[specialize_fn debug] f.name={:?} f.params[0].1={:?}",
                f.name, f.params[0].1
            );
            // Check if self is directly a generic type
            if let HirType::Generic(struct_sym, _) = f.params[0].1 {
                if let Some(struct_info) = self.find_struct(struct_sym) {
                    for (i, tp) in struct_info.type_params.iter().enumerate() {
                        if let Some(ct) = concrete.get(i) {
                            sub.insert(*tp, ct.clone());
                        }
                    }
                }
            }
            // Check if self is wrapped in RawPtr (e.g., *mut Iter<T>)
            else if let HirType::RawPtr(ref inner) = f.params[0].1 {
                if let HirType::Generic(struct_sym, _) = inner.as_ref() {
                    if let Some(struct_info) = self.find_struct(*struct_sym) {
                        for (i, tp) in struct_info.type_params.iter().enumerate() {
                            if let Some(ct) = concrete.get(i) {
                                sub.insert(*tp, ct.clone());
                            }
                        }
                    }
                }
            }
        }

        if f.type_params.is_empty() && sub.is_empty() {
            return f.clone();
        }

        for ct in concrete {
            ensure_struct_specialized(self, ct);
        }
        self.collect_type_overrides_for_expr(&f.body, &sub);
        self.scan_expr_for_generic_calls(&f.body, &sub);
        self.scan_expr_for_struct_instantiations(&f.body, &sub);

        let mut mono = f.clone();
        mono.type_params.clear();
        for (_, pt) in &mut mono.params {
            *pt = crate::types::substitute_type(pt, &sub);
        }
        if let Some(rt) = &mut mono.ret {
            *rt = crate::types::substitute_type(rt, &sub);
        }
        mono.body = self.substitute_expr_types(&mono.body, &sub);
        self.concretize_enum_variant_names(&mut mono.body, &sub);
        if !sub.is_empty() {
            mono.body = force_substitute_as_targets(mono.body, &sub);
        }
        self.scan_expr_for_generic_calls(&mono.body, &sub);
        self.scan_expr_for_struct_instantiations(&mono.body, &sub);

        mono
    }

    pub(crate) fn collect_type_overrides_for_expr(
        &mut self,
        expr: &HirExpr,
        sub: &HashMap<Symbol, HirType>,
    ) {
        let id = expr.get_id();
        if let Some(orig) = self.expr_types.get(id.as_usize()) {
            let new_ty = crate::types::substitute_type(orig, sub);
            if &new_ty != orig {
                self.type_overrides.insert(id, new_ty);
            }
        }
        match expr {
            HirExpr::Block { stmts, .. } => stmts
                .iter()
                .for_each(|s| self.collect_type_overrides_for_stmt(s, sub)),
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
                for arm in arms {
                    if let Some(g) = &arm.guard {
                        self.collect_type_overrides_for_expr(g, sub);
                    }
                    self.collect_type_overrides_for_expr(&arm.body, sub);
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

    pub(crate) fn collect_type_overrides_for_stmt(
        &mut self,
        stmt: &HirStmt,
        sub: &HashMap<Symbol, HirType>,
    ) {
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

    pub(crate) fn substitute_expr_types(
        &mut self,
        expr: &HirExpr,
        sub: &HashMap<Symbol, HirType>,
    ) -> HirExpr {
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
                    .map(|arm| MatchArm {
                        pattern: arm.pattern.clone(),
                        guard: arm
                            .guard
                            .as_ref()
                            .map(|g| self.substitute_expr_types(g, sub)),
                        body: self.substitute_expr_types(&arm.body, sub),
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
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                args,
                span,
                ..
            } => HirExpr::MethodCall {
                id: *id,
                receiver: Box::new(self.substitute_expr_types(receiver, sub)),
                method_name: *method_name,
                resolved_callee: None,
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
                op: *op,
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
            HirExpr::ForIn {
                id,
                pattern,
                iter,
                body,
                span,
            } => HirExpr::ForIn {
                id: *id,
                pattern: pattern.clone(),
                iter: Box::new(self.substitute_expr_types(iter, sub)),
                body: Box::new(self.substitute_expr_types(body, sub)),
                span: *span,
            },
            HirExpr::Deref { id, expr, span } => HirExpr::Deref {
                id: *id,
                expr: Box::new(self.substitute_expr_types(expr, sub)),
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
            other => other.clone(),
        }
    }

    pub(crate) fn substitute_stmt_types(
        &mut self,
        stmt: &HirStmt,
        sub: &HashMap<Symbol, HirType>,
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

    pub(crate) fn concretize_enum_variant_names(
        &mut self,
        expr: &mut HirExpr,
        sub: &HashMap<Symbol, HirType>,
    ) {
        match expr {
            HirExpr::EnumVariant {
                enum_name, args, ..
            } => {
                if let Some(edef) = self.find_enum(*enum_name) {
                    let concrete: Vec<HirType> = edef
                        .type_params
                        .iter()
                        .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Named(*tp)))
                        .collect();
                    if !concrete.is_empty()
                        && concrete.iter().all(|a| !self.has_unresolved_type_param(a))
                    {
                        *enum_name = self.mangle_name(*enum_name, &concrete);
                    }
                }
                for a in args {
                    self.concretize_enum_variant_names(a, sub);
                }
            }
            HirExpr::Block { stmts, .. } => {
                for s in stmts {
                    match s {
                        HirStmt::Expr(e)
                        | HirStmt::Let { value: e, .. }
                        | HirStmt::LetPat { value: e, .. }
                        | HirStmt::Assign { value: e, .. } => {
                            self.concretize_enum_variant_names(e, sub);
                        }
                        HirStmt::AssignField { object, value, .. } => {
                            self.concretize_enum_variant_names(object, sub);
                            self.concretize_enum_variant_names(value, sub);
                        }
                        HirStmt::AssignDeref { target, value, .. } => {
                            self.concretize_enum_variant_names(target, sub);
                            self.concretize_enum_variant_names(value, sub);
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
                self.concretize_enum_variant_names(condition, sub);
                self.concretize_enum_variant_names(then_branch, sub);
                if let Some(eb) = else_branch {
                    self.concretize_enum_variant_names(eb, sub);
                }
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                self.concretize_enum_variant_names(scrutinee, sub);
                for arm in arms {
                    if let Some(g) = &mut arm.guard {
                        self.concretize_enum_variant_names(g, sub);
                    }
                    self.concretize_enum_variant_names(&mut arm.body, sub);
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
                self.concretize_enum_variant_names(condition, sub);
                self.concretize_enum_variant_names(body, sub);
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.concretize_enum_variant_names(lhs, sub);
                self.concretize_enum_variant_names(rhs, sub);
            }
            HirExpr::Unary { operand, .. }
            | HirExpr::Deref { expr: operand, .. }
            | HirExpr::As { expr: operand, .. }
            | HirExpr::Return {
                value: Some(operand),
                ..
            }
            | HirExpr::Println { arg: operand, .. } => {
                self.concretize_enum_variant_names(operand, sub);
            }
            HirExpr::Assert {
                condition, message, ..
            } => {
                self.concretize_enum_variant_names(condition, sub);
                if let Some(m) = message {
                    self.concretize_enum_variant_names(m, sub);
                }
            }
            HirExpr::Call { args, .. }
            | HirExpr::MethodCall { args, .. }
            | HirExpr::TupleLit { elements: args, .. } => {
                for a in args {
                    self.concretize_enum_variant_names(a, sub);
                }
            }
            HirExpr::StructLit { fields, .. } => {
                for (_, val) in fields {
                    self.concretize_enum_variant_names(val, sub);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn specialize_struct(&mut self, s: &StructDef, concrete: &[HirType]) -> StructDef {
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

    pub(crate) fn specialize_enum(&mut self, e: &EnumDef, concrete: &[HirType]) -> EnumDef {
        let mut sub = HashMap::new();
        for (i, tp) in e.type_params.iter().enumerate() {
            if let Some(ct) = concrete.get(i) {
                sub.insert(*tp, ct.clone());
            }
        }
        let mut mono = e.clone();
        mono.type_params.clear();
        for variant in &mut mono.variants {
            for field in &mut variant.fields {
                field.ty = crate::types::substitute_type(&field.ty, &sub);
            }
        }
        mono
    }
}

fn ensure_struct_specialized(ctx: &mut MonoContext<'_>, ty: &HirType) {
    if let HirType::Generic(sym, args) = ty {
        if ctx.find_struct(*sym).is_some() {
            let concrete = args.clone();
            let key = (*sym, concrete.clone());
            if !ctx.struct_specs.contains_key(&key)
                && let Some(s) = ctx.find_struct(*sym)
            {
                let specialized = ctx.specialize_struct(&s, &concrete);
                ctx.struct_specs.insert(key, specialized);
            }
        }
        for arg in args {
            ensure_struct_specialized(ctx, arg);
        }
    }
}

fn force_substitute_as_targets(expr: HirExpr, sub: &HashMap<Symbol, HirType>) -> HirExpr {
    match expr {
        HirExpr::As {
            id,
            expr: inner,
            target_type,
            span,
        } => HirExpr::As {
            id,
            expr: Box::new(force_substitute_as_targets(*inner, sub)),
            target_type: crate::types::substitute_type(&target_type, sub),
            span,
        },
        other => other,
    }
}
