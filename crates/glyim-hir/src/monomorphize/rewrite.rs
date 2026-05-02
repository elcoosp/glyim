use super::*;
use crate::node::{HirExpr, HirStmt};
use crate::types::HirType;
use glyim_interner::Symbol;
use std::collections::HashMap;

impl<'a> MonoContext<'a> {
    pub(crate) fn substitute_expr_types(&mut self, expr: &HirExpr, sub: &HashMap<Symbol, HirType>) -> HirExpr {
        match expr {
            HirExpr::SizeOf { id, target_type, span } => HirExpr::SizeOf { id: *id, target_type: crate::types::substitute_type(target_type, sub), span: *span },
            HirExpr::As { id, expr: inner, target_type, span } => HirExpr::As { id: *id, expr: Box::new(self.substitute_expr_types(inner, sub)), target_type: crate::types::substitute_type(target_type, sub), span: *span },
            HirExpr::Block { id, stmts, span } => HirExpr::Block { id: *id, stmts: stmts.iter().map(|s| self.substitute_stmt_types(s, sub)).collect(), span: *span },
            HirExpr::Binary { id, op, lhs, rhs, span } => HirExpr::Binary { id: *id, op: op.clone(), lhs: Box::new(self.substitute_expr_types(lhs, sub)), rhs: Box::new(self.substitute_expr_types(rhs, sub)), span: *span },
            HirExpr::If { id, condition, then_branch, else_branch, span } => HirExpr::If { id: *id, condition: Box::new(self.substitute_expr_types(condition, sub)), then_branch: Box::new(self.substitute_expr_types(then_branch, sub)), else_branch: else_branch.as_ref().map(|e| Box::new(self.substitute_expr_types(e, sub))), span: *span },
            HirExpr::Match { id, scrutinee, arms, span } => HirExpr::Match { id: *id, scrutinee: Box::new(self.substitute_expr_types(scrutinee, sub)), arms: arms.iter().map(|(pat, guard, body)| (pat.clone(), guard.as_ref().map(|g| self.substitute_expr_types(g, sub)), self.substitute_expr_types(body, sub))).collect(), span: *span },
            HirExpr::Call { id, callee, args, span } => HirExpr::Call { id: *id, callee: *callee, args: args.iter().map(|a| self.substitute_expr_types(a, sub)).collect(), span: *span },
            HirExpr::MethodCall { id, receiver, method_name, args, span } => HirExpr::MethodCall { id: *id, receiver: Box::new(self.substitute_expr_types(receiver, sub)), method_name: *method_name, args: args.iter().map(|a| self.substitute_expr_types(a, sub)).collect(), span: *span },
            HirExpr::Unary { id, op, operand, span } => HirExpr::Unary { id: *id, op: op.clone(), operand: Box::new(self.substitute_expr_types(operand, sub)), span: *span },
            HirExpr::Return { id, value, span } => HirExpr::Return { id: *id, value: value.as_ref().map(|v| Box::new(self.substitute_expr_types(v, sub))), span: *span },
            HirExpr::StructLit { id, struct_name, fields, span } => HirExpr::StructLit { id: *id, struct_name: *struct_name, fields: fields.iter().map(|(s, e)| (*s, self.substitute_expr_types(e, sub))).collect(), span: *span },
            HirExpr::EnumVariant { id, enum_name, variant_name, args, span } => HirExpr::EnumVariant { id: *id, enum_name: *enum_name, variant_name: *variant_name, args: args.iter().map(|a| self.substitute_expr_types(a, sub)).collect(), span: *span },
            HirExpr::While { id, condition, body, span } => HirExpr::While { id: *id, condition: Box::new(self.substitute_expr_types(condition, sub)), body: Box::new(self.substitute_expr_types(body, sub)), span: *span },
            HirExpr::ForIn { id, pattern, iter, body, span } => HirExpr::ForIn { id: *id, pattern: pattern.clone(), iter: Box::new(self.substitute_expr_types(iter, sub)), body: Box::new(self.substitute_expr_types(body, sub)), span: *span },
            HirExpr::Deref { id, expr: inner, span } => HirExpr::Deref { id: *id, expr: Box::new(self.substitute_expr_types(inner, sub)), span: *span },
            HirExpr::AddrOf { id, target, span } => HirExpr::AddrOf { id: *id, target: *target, span: *span },
            HirExpr::FieldAccess { id, object, field, span } => HirExpr::FieldAccess { id: *id, object: Box::new(self.substitute_expr_types(object, sub)), field: *field, span: *span },
            HirExpr::TupleLit { id, elements, span } => HirExpr::TupleLit { id: *id, elements: elements.iter().map(|e| self.substitute_expr_types(e, sub)).collect(), span: *span },
            HirExpr::Println { id, arg, span } => HirExpr::Println { id: *id, arg: Box::new(self.substitute_expr_types(arg, sub)), span: *span },
            HirExpr::Assert { id, condition, message, span } => HirExpr::Assert { id: *id, condition: Box::new(self.substitute_expr_types(condition, sub)), message: message.as_ref().map(|m| Box::new(self.substitute_expr_types(m, sub))), span: *span },
            _ => expr.clone(),
        }
    }

    pub(crate) fn substitute_stmt_types(&mut self, stmt: &HirStmt, sub: &HashMap<Symbol, HirType>) -> HirStmt {
        match stmt {
            HirStmt::Let { name, mutable, value, span } => HirStmt::Let { name: *name, mutable: *mutable, value: self.substitute_expr_types(value, sub), span: *span },
            HirStmt::LetPat { pattern, mutable, value, span, ty } => HirStmt::LetPat { pattern: pattern.clone(), mutable: *mutable, value: self.substitute_expr_types(value, sub), ty: ty.clone(), span: *span },
            HirStmt::Assign { target, value, span } => HirStmt::Assign { target: *target, value: self.substitute_expr_types(value, sub), span: *span },
            HirStmt::AssignField { object, field, value, span } => HirStmt::AssignField { object: Box::new(self.substitute_expr_types(object, sub)), field: *field, value: self.substitute_expr_types(value, sub), span: *span },
            HirStmt::AssignDeref { target, value, span } => HirStmt::AssignDeref { target: Box::new(self.substitute_expr_types(target, sub)), value: self.substitute_expr_types(value, sub), span: *span },
            HirStmt::Expr(e) => HirStmt::Expr(self.substitute_expr_types(e, sub)),
        }
    }

    pub(crate) fn specialize_struct(&mut self, s: &StructDef, concrete: &[HirType]) -> StructDef {
        let mut sub = HashMap::new();
        for (i, tp) in s.type_params.iter().enumerate() { if let Some(ct) = concrete.get(i) { sub.insert(*tp, ct.clone()); } }
        let mut mono = s.clone();
        mono.type_params.clear();
        for field in &mut mono.fields { field.ty = crate::types::substitute_type(&field.ty, &sub); }
        mono
    }

    pub(crate) fn rewrite_fn(&mut self, f: &HirFn, fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>, struct_map: &HashMap<Symbol, Symbol>) -> HirFn {
        let mut mono = f.clone();
        mono.body = self.rewrite_expr(&f.body, fn_map, struct_map);
        mono
    }

    #[tracing::instrument(skip_all)]
    pub(crate) fn rewrite_expr(&mut self, expr: &HirExpr, fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>, struct_map: &HashMap<Symbol, Symbol>) -> HirExpr {
        match expr {
            HirExpr::Call { id, callee, args, span } => {
                let type_args = self.call_type_args.get(id).cloned().or_else(|| self.inferred_call_args.get(id).cloned()).unwrap_or_default();
                let type_args_empty = type_args.is_empty();
                let mut new_callee = fn_map.get(&(*callee, type_args.clone())).copied().unwrap_or(*callee);
                if new_callee == *callee && type_args_empty {
                    if let Some(((_, _), mono)) = fn_map.iter().find(|((sym, _), _)| *sym == *callee) { new_callee = *mono; }
                }
                HirExpr::Call { id: *id, callee: new_callee, args: args.iter().map(|a| self.rewrite_expr(a, fn_map, struct_map)).collect(), span: *span }
            }
            HirExpr::StructLit { id, struct_name, fields, span } => {
                let new_name = struct_map.get(struct_name).copied().unwrap_or(*struct_name);
                HirExpr::StructLit { id: *id, struct_name: new_name, fields: fields.iter().map(|(s, e)| (*s, self.rewrite_expr(e, fn_map, struct_map))).collect(), span: *span }
            }
            HirExpr::MethodCall { id, receiver, method_name, args, span } => {
                let rewritten_receiver = Box::new(self.rewrite_expr(receiver, fn_map, struct_map));
                let rewritten_args: Vec<HirExpr> = args.iter().map(|a| self.rewrite_expr(a, fn_map, struct_map)).collect();
                let receiver_ty = self.expr_types.get(receiver.get_id().as_usize());
                let inner_ty = match receiver_ty { Some(HirType::RawPtr(inner)) => Some(inner.as_ref().clone()), other => other.cloned() };
                if let Some(HirType::Named(type_name) | HirType::Generic(type_name, _)) = inner_ty {
                    let mangled = format!("{}_{}", self.interner.resolve(type_name), self.interner.resolve(*method_name));
                    let mangled_sym = self.interner.intern(&mangled);
                    let receiver_type_args: Vec<HirType> = match receiver_ty { Some(HirType::Generic(_, args)) => args.clone(), _ => vec![] };
                    let concrete_key = fn_map.iter().find_map(|((sym, args), mono_name)| if *sym == mangled_sym && *args == receiver_type_args { Some((args.clone(), *mono_name)) } else { None });
                    let concrete_key = concrete_key.or_else(|| {
                        if receiver_type_args.is_empty() { fn_map.iter().find(|((sym, _), _)| *sym == mangled_sym).map(|((_, args), mono_name)| (args.clone(), *mono_name)) } else { None }
                    });
                    if let Some(concrete_key) = concrete_key {
                        let mut all_args = vec![*rewritten_receiver.clone()];
                        all_args.extend(rewritten_args);
                        return HirExpr::Call { id: *id, callee: concrete_key.1, args: all_args, span: *span };
                    }
                }
                HirExpr::MethodCall { id: *id, receiver: rewritten_receiver, method_name: *method_name, args: rewritten_args, span: *span }
            }
            HirExpr::Block { id, stmts, span } => HirExpr::Block { id: *id, stmts: stmts.iter().map(|s| self.rewrite_stmt(s, fn_map, struct_map)).collect(), span: *span },
            HirExpr::If { id, condition, then_branch, else_branch, span } => HirExpr::If { id: *id, condition: Box::new(self.rewrite_expr(condition, fn_map, struct_map)), then_branch: Box::new(self.rewrite_expr(then_branch, fn_map, struct_map)), else_branch: else_branch.as_ref().map(|e| Box::new(self.rewrite_expr(e, fn_map, struct_map))), span: *span },
            HirExpr::Match { id, scrutinee, arms, span } => HirExpr::Match { id: *id, scrutinee: Box::new(self.rewrite_expr(scrutinee, fn_map, struct_map)), arms: arms.iter().map(|(pat, guard, body)| (pat.clone(), guard.as_ref().map(|g| Box::new(self.rewrite_expr(g, fn_map, struct_map))).map(|b| *b), self.rewrite_expr(body, fn_map, struct_map))).collect(), span: *span },
            HirExpr::Binary { id, op, lhs, rhs, span } => HirExpr::Binary { id: *id, op: op.clone(), lhs: Box::new(self.rewrite_expr(lhs, fn_map, struct_map)), rhs: Box::new(self.rewrite_expr(rhs, fn_map, struct_map)), span: *span },
            HirExpr::Unary { id, op, operand, span } => HirExpr::Unary { id: *id, op: op.clone(), operand: Box::new(self.rewrite_expr(operand, fn_map, struct_map)), span: *span },
            HirExpr::Return { id, value, span } => HirExpr::Return { id: *id, value: value.as_ref().map(|v| Box::new(self.rewrite_expr(v, fn_map, struct_map))), span: *span },
            HirExpr::Deref { id, expr, span } => HirExpr::Deref { id: *id, expr: Box::new(self.rewrite_expr(expr, fn_map, struct_map)), span: *span },
            HirExpr::AddrOf { id, target, span } => HirExpr::AddrOf { id: *id, target: *target, span: *span },
            HirExpr::ForIn { id, pattern, iter, body, span } => HirExpr::ForIn { id: *id, pattern: pattern.clone(), iter: Box::new(self.rewrite_expr(iter, fn_map, struct_map)), body: Box::new(self.rewrite_expr(body, fn_map, struct_map)), span: *span },
            HirExpr::While { id, condition, body, span } => HirExpr::While { id: *id, condition: Box::new(self.rewrite_expr(condition, fn_map, struct_map)), body: Box::new(self.rewrite_expr(body, fn_map, struct_map)), span: *span },
            HirExpr::FieldAccess { id, object, field, span } => HirExpr::FieldAccess { id: *id, object: Box::new(self.rewrite_expr(object, fn_map, struct_map)), field: *field, span: *span },
            _ => expr.clone(),
        }
    }

    #[tracing::instrument(skip_all)]
    pub(crate) fn rewrite_stmt(&mut self, stmt: &HirStmt, fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>, struct_map: &HashMap<Symbol, Symbol>) -> HirStmt {
        match stmt {
            HirStmt::Let { name, mutable, value, span } => HirStmt::Let { name: *name, mutable: *mutable, value: self.rewrite_expr(value, fn_map, struct_map), span: *span },
            HirStmt::LetPat { pattern, mutable, value, span, ty } => HirStmt::LetPat { pattern: pattern.clone(), mutable: *mutable, value: self.rewrite_expr(value, fn_map, struct_map), ty: ty.clone(), span: *span },
            HirStmt::Assign { target, value, span } => HirStmt::Assign { target: *target, value: self.rewrite_expr(value, fn_map, struct_map), span: *span },
            HirStmt::AssignDeref { target, value, span } => HirStmt::AssignDeref { target: Box::new(self.rewrite_expr(target, fn_map, struct_map)), value: self.rewrite_expr(value, fn_map, struct_map), span: *span },
            HirStmt::AssignField { object, field, value, span } => HirStmt::AssignField { object: Box::new(self.rewrite_expr(object, fn_map, struct_map)), field: *field, value: self.rewrite_expr(value, fn_map, struct_map), span: *span },
            HirStmt::Expr(e) => HirStmt::Expr(self.rewrite_expr(e, fn_map, struct_map)),
        }
    }
}
