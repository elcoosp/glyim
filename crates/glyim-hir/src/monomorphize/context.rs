use crate::monomorphize::mangle_table::MangleTable;
// crates/glyim-hir/src/monomorphize/context.rs
use super::*;
use crate::HirPattern;
use crate::item::{EnumDef, HirItem};
use crate::node::MatchArm;
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
            call_type_args_overrides: HashMap::new(),
            mangle_table: MangleTable::new(),
            enum_specs: HashMap::new(),
            type_work_queue: Vec::new(),
            type_queued: HashSet::new(),
        }
    }

    /// Get the type for an expression, checking type_overrides first (specialized types)
    /// then falling back to expr_types (original types from typeck).
    pub(crate) fn get_expr_type(&self, id: ExprId) -> Option<HirType> {
        self.type_overrides
            .get(&id)
            .cloned()
            .or_else(|| self.expr_types.get(id.as_usize()).cloned())
    }

    pub(crate) fn find_fn(&mut self, name: Symbol) -> Option<HirFn> {
        let name_str = self.interner.resolve(name).to_string();

        // First check if there's already a specialization with no type params
        // (e.g., Vec_new__i64 for Vec_new called with [Int])
        if let Some(pos) = name_str.find("__") {
            let base_name = self.interner.intern(&name_str[..pos]);
            // Check if the fully-specialized function exists in any impl
            let base_str = self.interner.resolve(base_name).to_string();
            if let Some(us_pos) = base_str.rfind('_') {
                let type_name = &base_str[..us_pos];
                let method_name = &base_str[us_pos + 1..];
                if type_name.starts_with(|c: char| c.is_uppercase()) {
                    let type_sym = self.interner.intern(type_name);
                    let method_sym = self.interner.intern(method_name);
                    for item in &self.hir.items {
                        if let HirItem::Impl(imp) = item {
                            if imp.target_name == type_sym {
                                for m in &imp.methods {
                                    if m.name == method_sym && m.type_params.is_empty() {
                                        // The method is already fully specialized
                                        return Some(m.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Fall back to the generic version
            return self.find_fn(base_name);
        }

        // Search top-level functions
        for item in &self.hir.items {
            if let HirItem::Fn(f) = item
                && f.name == name
            {
                return Some(f.clone());
            }
        }

        // Search impl methods with exact name match
        for item in &self.hir.items {
            if let HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    if m.name == name {
                        return Some(m.clone());
                    }
                }
            }
        }

        // Search impl methods by demangled name (e.g., Vec_new -> Vec::new)
        // The HIR stores impl methods with their original short names (new, inc_len),
        // but call sites use mangled names (Vec_new, Vec_inc_len).
        for item in &self.hir.items {
            if let HirItem::Impl(imp) = item {
                let type_name = self.interner.resolve(imp.target_name);
                for m in &imp.methods {
                    let method_name = self.interner.resolve(m.name);
                    let mangled_form = format!("{}_{}", type_name, method_name);
                    if mangled_form == name_str {
                        return Some(m.clone());
                    }
                }
            }
        }

        // Try to parse as impl method name (e.g., "Vec_new", "HashMap_hash")
        // Use rfind to get the last underscore
        if let Some(underscore_pos) = name_str.rfind('_') {
            let potential_type_name = &name_str[..underscore_pos];
            let potential_method_name = &name_str[underscore_pos + 1..];

            // Try looking up in impl methods for the type
            if potential_type_name.starts_with(|c: char| c.is_uppercase()) {
                let type_sym = self.interner.intern(potential_type_name);
                let method_sym = self.interner.intern(potential_method_name);

                for item in &self.hir.items {
                    if let HirItem::Impl(imp) = item {
                        if imp.target_name == type_sym {
                            for m in &imp.methods {
                                if m.name == method_sym {
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

    pub(crate) fn mangle_name(&mut self, base: Symbol, type_args: &[HirType]) -> Symbol {
        self.mangle_table.mangle(base, type_args, self.interner)
    }

    /// Check if a type contains any unresolved type parameters (single uppercase letters)
    /// Check whether the body of a function depends on the given type parameters
    /// in a size-critical way (e.g., SizeOf, field access with unknown layout).
    pub(crate) fn body_depends_on_type_params(
        &self,
        body: &crate::node::HirExpr,
        type_params: &[glyim_interner::Symbol],
    ) -> bool {
        use crate::node::{HirExpr, HirStmt};
        fn expr_depends(
            expr: &HirExpr,
            type_params: &[glyim_interner::Symbol],
            interner: &glyim_interner::Interner,
        ) -> bool {
            match expr {
                HirExpr::SizeOf { target_type, .. } => {
                    MonoContext::type_refers_to_params(target_type, type_params, interner)
                }
                HirExpr::As { target_type, .. } => {
                    MonoContext::type_refers_to_params(target_type, type_params, interner)
                }
                HirExpr::Block { stmts, .. } => {
                    stmts.iter().any(|s| stmt_depends(s, type_params, interner))
                }
                HirExpr::If {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    expr_depends(condition, type_params, interner)
                        || expr_depends(then_branch, type_params, interner)
                        || else_branch
                            .as_ref()
                            .map_or(false, |e| expr_depends(e, type_params, interner))
                }
                HirExpr::While {
                    condition, body, ..
                } => {
                    expr_depends(condition, type_params, interner)
                        || expr_depends(body, type_params, interner)
                }
                HirExpr::Match {
                    scrutinee, arms, ..
                } => {
                    expr_depends(scrutinee, type_params, interner)
                        || arms.iter().any(|arm| {
                            arm.guard
                                .as_ref()
                                .map_or(false, |g| expr_depends(g, type_params, interner))
                                || expr_depends(&arm.body, type_params, interner)
                        })
                }
                HirExpr::FieldAccess {
                    object, field: _, ..
                } => expr_depends(object, type_params, interner),
                HirExpr::Call {
                    callee: _, args, ..
                } => args.iter().any(|a| expr_depends(a, type_params, interner)),
                HirExpr::MethodCall {
                    receiver,
                    method_name: _,
                    args,
                    ..
                } => {
                    expr_depends(receiver, type_params, interner)
                        || args.iter().any(|a| expr_depends(a, type_params, interner))
                }
                HirExpr::Binary { lhs, rhs, .. } => {
                    expr_depends(lhs, type_params, interner)
                        || expr_depends(rhs, type_params, interner)
                }
                HirExpr::Unary { operand, .. } | HirExpr::Deref { expr: operand, .. } => {
                    expr_depends(operand, type_params, interner)
                }
                HirExpr::Return { value, .. } => value
                    .as_ref()
                    .map_or(false, |v| expr_depends(v, type_params, interner)),
                HirExpr::StructLit { fields, .. } => fields
                    .iter()
                    .any(|(_, e)| expr_depends(e, type_params, interner)),
                HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => {
                    args.iter().any(|a| expr_depends(a, type_params, interner))
                }
                HirExpr::ForIn { iter, body, .. } => {
                    expr_depends(iter, type_params, interner)
                        || expr_depends(body, type_params, interner)
                }
                _ => false,
            }
        }
        fn stmt_depends(
            stmt: &HirStmt,
            type_params: &[glyim_interner::Symbol],
            interner: &glyim_interner::Interner,
        ) -> bool {
            match stmt {
                HirStmt::Let { value, .. } | HirStmt::LetPat { value, .. } => {
                    expr_depends(value, type_params, interner)
                }
                HirStmt::Assign { value, .. } => expr_depends(value, type_params, interner),
                HirStmt::AssignField { object, value, .. } => {
                    expr_depends(object, type_params, interner)
                        || expr_depends(value, type_params, interner)
                }
                HirStmt::AssignDeref { target, value, .. } => {
                    expr_depends(target, type_params, interner)
                        || expr_depends(value, type_params, interner)
                }
                HirStmt::Expr(e) => expr_depends(e, type_params, interner),
            }
        }
        expr_depends(body, type_params, self.interner)
    }

    /// Check if a type references any of the given type parameter symbols.
    pub(crate) fn type_refers_to_params(
        ty: &HirType,
        type_params: &[glyim_interner::Symbol],
        interner: &glyim_interner::Interner,
    ) -> bool {
        match ty {
            HirType::Named(sym) => type_params.contains(sym),
            HirType::Generic(sym, args) => {
                type_params.contains(sym)
                    || args
                        .iter()
                        .any(|a| MonoContext::type_refers_to_params(a, type_params, interner))
            }
            HirType::Tuple(elems) => elems
                .iter()
                .any(|e| MonoContext::type_refers_to_params(e, type_params, interner)),
            HirType::RawPtr(inner) => {
                MonoContext::type_refers_to_params(inner, type_params, interner)
            }
            HirType::Option(inner) => {
                MonoContext::type_refers_to_params(inner, type_params, interner)
            }
            HirType::Result(ok, err) => {
                MonoContext::type_refers_to_params(ok, type_params, interner)
                    || MonoContext::type_refers_to_params(err, type_params, interner)
            }
            _ => false,
        }
    }

    pub(crate) fn has_unresolved_type_param(&self, ty: &HirType) -> bool {
        match ty {
            HirType::Named(sym) => {
                let s = self.interner.resolve(*sym);
                s.len() == 1 && s.chars().next().map_or(false, |c| c.is_uppercase())
            }
            HirType::Generic(_, args) => args.iter().any(|a| self.has_unresolved_type_param(a)),
            HirType::RawPtr(inner) => self.has_unresolved_type_param(inner.as_ref()),
            HirType::Option(inner) => self.has_unresolved_type_param(inner.as_ref()),
            HirType::Result(ok, err) => {
                self.has_unresolved_type_param(ok) || self.has_unresolved_type_param(err)
            }
            HirType::Tuple(elems) => elems.iter().any(|e| self.has_unresolved_type_param(e)),
            _ => false,
        }
    }

    /// Attempt to infer type args for a zero-argument generic call by looking
    /// at later calls on the same variable within the enclosing block.
    pub(crate) fn infer_from_same_var_in_block(
        &self,
        callee: &Symbol,
        call_id: ExprId,
        type_params: &[Symbol],
    ) -> Option<Vec<HirType>> {
        for item in &self.hir.items {
            if let HirItem::Fn(fn_def) = item {
                if let Some(result) = self.find_in_block(&fn_def.body, callee, call_id, type_params)
                {
                    return Some(result);
                }
            }
        }
        None
    }

    fn find_in_block(
        &self,
        expr: &HirExpr,
        callee: &Symbol,
        target_call_id: ExprId,
        type_params: &[Symbol],
    ) -> Option<Vec<HirType>> {
        match expr {
            HirExpr::Block { stmts, .. } => {
                let mut found = false;
                let mut target_var: Option<Symbol> = None;
                for stmt in stmts {
                    match stmt {
                        HirStmt::Let { name, value, .. } if value.get_id() == target_call_id => {
                            found = true;
                            target_var = Some(*name);
                            continue;
                        }
                        HirStmt::LetPat {
                            pattern: HirPattern::Var(name),
                            value,
                            ..
                        } if value.get_id() == target_call_id => {
                            found = true;
                            target_var = Some(*name);
                            continue;
                        }
                        _ => {
                            if found && let Some(var_sym) = target_var {
                                if let HirStmt::Expr(inner) = stmt {
                                    if let Some(args) = self.extract_type_args_from_call_on_var(
                                        inner,
                                        *callee,
                                        var_sym,
                                        type_params,
                                    ) {
                                        return Some(args);
                                    }
                                }
                            }
                        }
                    }
                }
                None
            }
            HirExpr::If {
                then_branch,
                else_branch,
                ..
            } => self
                .find_in_block(then_branch, callee, target_call_id, type_params)
                .or_else(|| {
                    else_branch
                        .as_ref()
                        .and_then(|e| self.find_in_block(e, callee, target_call_id, type_params))
                }),
            HirExpr::While { body, .. } | HirExpr::ForIn { body, .. } => {
                self.find_in_block(body, callee, target_call_id, type_params)
            }
            _ => None,
        }
    }

    fn extract_type_args_from_call_on_var(
        &self,
        expr: &HirExpr,
        _callee: Symbol,
        var_sym: Symbol,
        _type_params: &[Symbol],
    ) -> Option<Vec<HirType>> {
        match expr {
            HirExpr::Call { args, .. } => {
                if !args.is_empty() && {
                    match &args[0] {
                        HirExpr::Ident { name, .. } => *name == var_sym,
                        _ => false,
                    }
                } {
                    if let Some(cached) = self.call_type_args.get(&expr.get_id()) {
                        if !cached.is_empty() {
                            return Some(cached.clone());
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Substitute unresolved type params in a type list using the given substitution
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
                    .map(|arm| {
                        let pat = &arm.pattern;
                        let guard = &arm.guard;
                        let body = &arm.body;
                        MatchArm {
                            pattern: pat.clone(),
                            guard: guard.as_ref().map(|g| self.substitute_expr_types(g, sub)),
                            body: self.substitute_expr_types(body, sub),
                        }
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
            } => {
                let new_name = if let Some(struct_def) = self.find_struct(*struct_name) {
                    if !struct_def.type_params.is_empty() && !sub.is_empty() {
                        let concrete: Vec<HirType> = struct_def
                            .type_params
                            .iter()
                            .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Named(*tp)))
                            .collect();
                        let all_concrete =
                            concrete.iter().all(|a| !self.has_unresolved_type_param(a));
                        if all_concrete {
                            self.mangle_name(*struct_name, &concrete)
                        } else {
                            *struct_name
                        }
                    } else {
                        *struct_name
                    }
                } else {
                    *struct_name
                };
                HirExpr::StructLit {
                    id: *id,
                    struct_name: new_name,
                    fields: fields
                        .iter()
                        .map(|(s, e)| (*s, self.substitute_expr_types(e, sub)))
                        .collect(),
                    span: *span,
                }
            }
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
            _ => expr.clone(),
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
}
