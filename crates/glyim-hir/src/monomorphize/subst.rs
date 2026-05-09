//! Single-pass type substitution and expression walker.
//!
//! This is the core of the new monomorphizer. For each expression it:
//! 1. Computes the concrete type (substitute + concretize)
//! 2. Assigns a fresh, globally-unique ExprId
//! 3. Stores the concrete type in output_expr_types
//! 4. Rewrites names (callee, struct_name, enum_name) to mangled forms
//! 5. Discovers new specializations and adds to `discovered`
//!
//! No post-hoc concretization pass is needed. The output is fully concrete.

use crate::monomorphize::concretize;
use crate::monomorphize::discover;
use crate::monomorphize::index::MonoIndex;
use crate::monomorphize::mangle_table::MangleTable;
use crate::monomorphize::pattern;
use crate::monomorphize::work::WorkItem;
use crate::node::{HirExpr, HirStmt, MatchArm};
use crate::types::{ExprId, HirPattern, HirType};
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

/// Context for a single substitution pass over one function body.
pub struct SubstContext<'a> {
    pub interner: &'a mut Interner,
    pub input_expr_types: &'a [HirType],
    pub call_type_args: &'a HashMap<ExprId, Vec<HirType>>,
    pub index: &'a MonoIndex,
    pub mangle_table: &'a mut MangleTable,
    next_expr_id: u32,
    start_id: u32,
    pub output_expr_types: Vec<HirType>,
    pub discovered: Vec<WorkItem>,
}

impl<'a> SubstContext<'a> {
    pub fn new(
        start_id: u32,
        interner: &'a mut Interner,
        input_expr_types: &'a [HirType],
        call_type_args: &'a HashMap<ExprId, Vec<HirType>>,
        index: &'a MonoIndex,
        mangle_table: &'a mut MangleTable,
    ) -> Self {
        Self {
            interner,
            input_expr_types,
            call_type_args,
            index,
            mangle_table,
            next_expr_id: start_id,
            start_id,
            output_expr_types: Vec::new(),
            discovered: Vec::new(),
        }
    }

    pub fn next_expr_id(&self) -> u32 {
        self.next_expr_id
    }

    fn fresh_id(&mut self) -> ExprId {
        let id = ExprId::new(self.next_expr_id);
        self.next_expr_id += 1;
        id
    }

    fn store_type(&mut self, id: ExprId, ty: HirType) {
        let idx = id.as_usize();
        if idx >= self.output_expr_types.len() {
            self.output_expr_types.resize(idx + 1, HirType::Error);
        }
        self.output_expr_types[idx] = ty;
    }

    fn concrete_type_for(
        &mut self,
        original_id: ExprId,
        sub: &HashMap<Symbol, HirType>,
    ) -> HirType {
        let orig_ty = self
            .input_expr_types
            .get(original_id.as_usize())
            .cloned()
            .unwrap_or(HirType::Error);
        concretize::substitute_and_concretize(
            &orig_ty,
            sub,
            self.index,
            self.mangle_table,
            self.interner,
        )
    }

    fn discover_from_type(&mut self, ty: &HirType) {
        let items = discover::discover_type_specializations(ty, self.index, self.interner);
        self.discovered.extend(items);
    }

    pub fn substitute_expr(
        &mut self,
        expr: &HirExpr,
        sub: &HashMap<Symbol, HirType>,
    ) -> HirExpr {
        let original_id = expr.get_id();
        let new_id = self.fresh_id();
        let concrete_ty = self.concrete_type_for(original_id, sub);
        self.store_type(new_id, concrete_ty.clone());
        self.discover_from_type(&concrete_ty);
        let span = expr.get_span();

        match expr {
            HirExpr::IntLit { value, .. } => HirExpr::IntLit { id: new_id, value: *value, span },
            HirExpr::FloatLit { value, .. } => HirExpr::FloatLit { id: new_id, value: *value, span },
            HirExpr::BoolLit { value, .. } => HirExpr::BoolLit { id: new_id, value: *value, span },
            HirExpr::StrLit { value, .. } => HirExpr::StrLit { id: new_id, value: value.clone(), span },
            HirExpr::UnitLit { .. } => HirExpr::UnitLit { id: new_id, span },
            HirExpr::Ident { name, .. } => HirExpr::Ident { id: new_id, name: *name, span },
            HirExpr::AddrOf { target, .. } => HirExpr::AddrOf { id: new_id, target: *target, span },

            HirExpr::Binary { op, lhs, rhs, .. } => {
                let new_lhs = Box::new(self.substitute_expr(lhs, sub));
                let new_rhs = Box::new(self.substitute_expr(rhs, sub));
                HirExpr::Binary { id: new_id, op: *op, lhs: new_lhs, rhs: new_rhs, span }
            }
            HirExpr::Unary { op, operand, .. } => {
                let new_operand = Box::new(self.substitute_expr(operand, sub));
                HirExpr::Unary { id: new_id, op: *op, operand: new_operand, span }
            }
            HirExpr::Block { stmts, .. } => {
                let new_stmts: Vec<_> = stmts.iter().map(|s| self.substitute_stmt(s, sub)).collect();
                HirExpr::Block { id: new_id, stmts: new_stmts, span }
            }
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                let new_cond = Box::new(self.substitute_expr(condition, sub));
                let new_then = Box::new(self.substitute_expr(then_branch, sub));
                let new_else = else_branch.as_ref().map(|e| Box::new(self.substitute_expr(e, sub)));
                HirExpr::If { id: new_id, condition: new_cond, then_branch: new_then, else_branch: new_else, span }
            }
            HirExpr::Call { callee, args, .. } => {
                self.substitute_call(new_id, original_id, *callee, args, span, sub)
            }
            HirExpr::MethodCall { receiver, method_name, args, .. } => {
                self.substitute_method_call(new_id, original_id, receiver, *method_name, args, span, sub)
            }
            HirExpr::StructLit { struct_name, fields, .. } => {
                self.substitute_struct_lit(new_id, *struct_name, fields, span, sub, &concrete_ty)
            }
            HirExpr::EnumVariant { enum_name, variant_name, args, .. } => {
                self.substitute_enum_variant(new_id, *enum_name, *variant_name, args, span, sub, &concrete_ty)
            }
            HirExpr::FieldAccess { object, field, .. } => {
                let new_obj = Box::new(self.substitute_expr(object, sub));
                HirExpr::FieldAccess { id: new_id, object: new_obj, field: *field, span }
            }
            HirExpr::TupleLit { elements, .. } => {
                let new_elems: Vec<_> = elements.iter().map(|e| self.substitute_expr(e, sub)).collect();
                HirExpr::TupleLit { id: new_id, elements: new_elems, span }
            }
            HirExpr::Deref { expr: inner, .. } => {
                let new_inner = Box::new(self.substitute_expr(inner, sub));
                HirExpr::Deref { id: new_id, expr: new_inner, span }
            }
            HirExpr::As { expr: inner, target_type, .. } => {
                let new_inner = Box::new(self.substitute_expr(inner, sub));
                let new_target = concretize::substitute_and_concretize(
                    target_type, sub, self.index, self.mangle_table, self.interner,
                );
                self.discover_from_type(&new_target);
                HirExpr::As { id: new_id, expr: new_inner, target_type: new_target, span }
            }
            HirExpr::SizeOf { target_type, .. } => {
                let new_target = concretize::substitute_and_concretize(
                    target_type, sub, self.index, self.mangle_table, self.interner,
                );
                self.discover_from_type(&new_target);
                HirExpr::SizeOf { id: new_id, target_type: new_target, span }
            }
            HirExpr::While { condition, body, .. } => {
                let new_cond = Box::new(self.substitute_expr(condition, sub));
                let new_body = Box::new(self.substitute_expr(body, sub));
                HirExpr::While { id: new_id, condition: new_cond, body: new_body, span }
            }
            HirExpr::ForIn { pattern: for_pat, iter, body, .. } => {
                self.substitute_forin(new_id, original_id, for_pat, iter, body, span, sub)
            }
            HirExpr::Return { value, .. } => {
                let new_val = value.as_ref().map(|v| Box::new(self.substitute_expr(v, sub)));
                HirExpr::Return { id: new_id, value: new_val, span }
            }
            HirExpr::Println { arg, .. } => {
                let new_arg = Box::new(self.substitute_expr(arg, sub));
                HirExpr::Println { id: new_id, arg: new_arg, span }
            }
            HirExpr::Assert { condition, message, .. } => {
                let new_cond = Box::new(self.substitute_expr(condition, sub));
                let new_msg = message.as_ref().map(|m| Box::new(self.substitute_expr(m, sub)));
                HirExpr::Assert { id: new_id, condition: new_cond, message: new_msg, span }
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                self.substitute_match(new_id, scrutinee, arms, span, sub, &concrete_ty)
            }
        }
    }

    fn substitute_call(
        &mut self,
        new_id: ExprId,
        original_id: ExprId,
        callee: Symbol,
        args: &[HirExpr],
        span: glyim_diag::Span,
        sub: &HashMap<Symbol, HirType>,
    ) -> HirExpr {
        let (new_callee, discovered) = discover::discover_call_specialization(
            original_id, callee, self.call_type_args, sub,
            self.index, self.mangle_table, self.interner,
        );
        self.discovered.extend(discovered);
        let new_args: Vec<_> = args.iter().map(|a| self.substitute_expr(a, sub)).collect();
        HirExpr::Call { id: new_id, callee: new_callee, args: new_args, span }
    }

    fn substitute_method_call(
        &mut self,
        new_id: ExprId,
        original_id: ExprId,
        receiver: &HirExpr,
        method_name: Symbol,
        args: &[HirExpr],
        span: glyim_diag::Span,
        sub: &HashMap<Symbol, HirType>,
    ) -> HirExpr {
        let new_receiver = Box::new(self.substitute_expr(receiver, sub));
        let new_args: Vec<HirExpr> = args.iter().map(|a| self.substitute_expr(a, sub)).collect();

        let resolved = discover::discover_method_call_specialization(
            original_id, receiver.get_id(), method_name,
            self.input_expr_types, self.call_type_args, sub,
            self.index, self.mangle_table, self.interner,
        );

        if let Some((mangled_callee, discovered)) = resolved {
            self.discovered.extend(discovered);
            let mut call_args = vec![*new_receiver];
            call_args.extend(new_args);
            return HirExpr::Call { id: new_id, callee: mangled_callee, args: call_args, span };
        }

        HirExpr::MethodCall {
            id: new_id,
            receiver: new_receiver,
            method_name,
            resolved_callee: None,
            args: new_args,
            span,
        }
    }

    fn substitute_struct_lit(
        &mut self,
        new_id: ExprId,
        struct_name: Symbol,
        fields: &[(Symbol, HirExpr)],
        span: glyim_diag::Span,
        sub: &HashMap<Symbol, HirType>,
        concrete_ty: &HirType,
    ) -> HirExpr {
        let new_name = match concrete_ty {
            HirType::Named(mangled) => *mangled,
            HirType::Generic(base, args) => {
                self.discovered.extend(discover::discover_type_specializations(concrete_ty, self.index, self.interner));
                self.mangle_table.mangle(*base, args, self.interner)
            }
            _ => {
                if let Some(sdef) = self.index.find_struct(struct_name) {
                    if sdef.type_params.is_empty() {
                        struct_name
                    } else {
                        let concrete_args: Vec<HirType> = sdef.type_params.iter().map(|tp| {
                            concretize::substitute_and_concretize(&HirType::Named(*tp), sub, self.index, self.mangle_table, self.interner)
                        }).collect();
                        self.mangle_table.mangle(struct_name, &concrete_args, self.interner)
                    }
                } else {
                    struct_name
                }
            }
        };

        let new_fields: Vec<_> = fields.iter().map(|(sym, val)| (*sym, self.substitute_expr(val, sub))).collect();
        HirExpr::StructLit { id: new_id, struct_name: new_name, fields: new_fields, span }
    }

    fn substitute_enum_variant(
        &mut self,
        new_id: ExprId,
        enum_name: Symbol,
        variant_name: Symbol,
        args: &[HirExpr],
        span: glyim_diag::Span,
        sub: &HashMap<Symbol, HirType>,
        concrete_ty: &HirType,
    ) -> HirExpr {
        let new_name = match concrete_ty {
            HirType::Named(mangled) => *mangled,
            HirType::Generic(base, args) => {
                self.discovered.extend(discover::discover_type_specializations(concrete_ty, self.index, self.interner));
                self.mangle_table.mangle(*base, args, self.interner)
            }
            _ => {
                if let Some(edef) = self.index.find_enum(enum_name) {
                    if edef.type_params.is_empty() {
                        enum_name
                    } else {
                        let concrete_args: Vec<HirType> = edef.type_params.iter().map(|tp| {
                            concretize::substitute_and_concretize(&HirType::Named(*tp), sub, self.index, self.mangle_table, self.interner)
                        }).collect();
                        self.mangle_table.mangle(enum_name, &concrete_args, self.interner)
                    }
                } else {
                    enum_name
                }
            }
        };
        let new_args: Vec<_> = args.iter().map(|a| self.substitute_expr(a, sub)).collect();
        HirExpr::EnumVariant { id: new_id, enum_name: new_name, variant_name, args: new_args, span }
    }

    fn substitute_match(
        &mut self,
        new_id: ExprId,
        scrutinee: &HirExpr,
        arms: &[MatchArm],
        span: glyim_diag::Span,
        sub: &HashMap<Symbol, HirType>,
        concrete_ty: &HirType,
    ) -> HirExpr {
        let new_scrutinee = Box::new(self.substitute_expr(scrutinee, sub));
        let scrutinee_concrete = self.input_expr_types.get(scrutinee.get_id().as_usize())
            .map(|t| concretize::substitute_and_concretize(t, sub, self.index, self.mangle_table, self.interner))
            .unwrap_or_else(|| concrete_ty.clone());

        let new_arms: Vec<MatchArm> = arms.iter().map(|arm| {
            let new_pat = self.substitute_pattern(&arm.pattern, &scrutinee_concrete);
            let new_guard = arm.guard.as_ref().map(|g| self.substitute_expr(g, sub));
            let new_body = self.substitute_expr(&arm.body, sub);
            MatchArm { pattern: new_pat, guard: new_guard, body: new_body }
        }).collect();

        HirExpr::Match { id: new_id, scrutinee: new_scrutinee, arms: new_arms, span }
    }

    fn substitute_forin(
        &mut self,
        new_id: ExprId,
        original_id: ExprId,
        for_pattern: &HirPattern,
        iter: &HirExpr,
        body: &HirExpr,
        span: glyim_diag::Span,
        sub: &HashMap<Symbol, HirType>,
    ) -> HirExpr {
        let new_iter = Box::new(self.substitute_expr(iter, sub));
        let new_body = Box::new(self.substitute_expr(body, sub));
        let forin_discoveries = discover::discover_forin_specializations(
            iter.get_id(), self.input_expr_types, sub,
            self.index, self.mangle_table, self.interner,
        );
        self.discovered.extend(forin_discoveries);
        HirExpr::ForIn { id: new_id, pattern: for_pattern.clone(), iter: new_iter, body: new_body, span }
    }

    fn substitute_pattern(&mut self, pat: &HirPattern, scrutinee_ty: &HirType) -> HirPattern {
        pattern::substitute_pattern(pat, scrutinee_ty, self)
    }

    pub fn substitute_stmt(&mut self, stmt: &HirStmt, sub: &HashMap<Symbol, HirType>) -> HirStmt {
        match stmt {
            HirStmt::Let { name, mutable, value, span } => {
                let new_val = self.substitute_expr(value, sub);
                HirStmt::Let { name: *name, mutable: *mutable, value: new_val, span: *span }
            }
            HirStmt::LetPat { pattern, mutable, value, ty, span } => {
                let new_val = self.substitute_expr(value, sub);
                let new_ty = ty.as_ref().map(|t| concretize::substitute_and_concretize(t, sub, self.index, self.mangle_table, self.interner));
                HirStmt::LetPat { pattern: pattern.clone(), mutable: *mutable, value: new_val, ty: new_ty, span: *span }
            }
            HirStmt::Assign { target, value, span } => {
                let new_val = self.substitute_expr(value, sub);
                HirStmt::Assign { target: *target, value: new_val, span: *span }
            }
            HirStmt::AssignField { object, field, value, span } => {
                let new_obj = Box::new(self.substitute_expr(object, sub));
                let new_val = self.substitute_expr(value, sub);
                HirStmt::AssignField { object: new_obj, field: *field, value: new_val, span: *span }
            }
            HirStmt::AssignDeref { target, value, span } => {
                let new_target = Box::new(self.substitute_expr(target, sub));
                let new_val = self.substitute_expr(value, sub);
                HirStmt::AssignDeref { target: new_target, value: new_val, span: *span }
            }
            HirStmt::Expr(e) => HirStmt::Expr(self.substitute_expr(e, sub)),
        }
    }
}

impl<'a> pattern::MangleContext for SubstContext<'a> {
    fn mangle_name(&mut self, base: Symbol, args: &[HirType]) -> Symbol {
        self.mangle_table.mangle(base, args, self.interner)
    }
    fn concretize_type(&mut self, ty: HirType) -> HirType {
        concretize::concretize_type(ty, self.index, self.mangle_table, self.interner)
    }
    fn intern_str(&mut self, s: &str) -> Symbol {
        self.interner.intern(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{StructDef, StructField};
    use crate::monomorphize::index::MonoIndex;
    use crate::monomorphize::mangle_table::MangleTable;
    use crate::monomorphize::work::ItemKind;
    use crate::node::HirFn;
    use crate::types::HirType;
    use glyim_diag::Span;
    use glyim_interner::Interner;

    fn build_index_with_id_fn(interner: &mut Interner) -> MonoIndex {
        let id_sym = interner.intern("id");
        let t_sym = interner.intern("T");
        let hir = crate::Hir {
            items: vec![crate::HirItem::Fn(HirFn {
                doc: None, name: id_sym, type_params: vec![t_sym],
                params: vec![(interner.intern("x"), HirType::Named(t_sym))],
                param_mutability: vec![false], ret: Some(HirType::Named(t_sym)),
                body: crate::node::HirExpr::Ident { id: ExprId::new(1), name: interner.intern("x"), span: Span::new(0,0) },
                span: Span::new(0,0), is_pub: false, is_macro_generated: false,
                is_extern_backed: false, is_test: false, test_config: None,
            })],
        };
        MonoIndex::build(&hir)
    }

    #[test]
    fn substitute_call_rewrites_generic_callee() {
        let mut interner = Interner::new();
        let id_sym = interner.intern("id");
        let index = build_index_with_id_fn(&mut interner);
        let mut mangle_table = MangleTable::new();
        let call_expr = HirExpr::Call { id: ExprId::new(0), callee: id_sym, args: vec![HirExpr::IntLit { id: ExprId::new(1), value: 42, span: Span::new(0,0) }], span: Span::new(0,0) };
        let mut call_type_args = HashMap::new();
        call_type_args.insert(ExprId::new(0), vec![HirType::Int]);
        let expr_types = vec![HirType::Int, HirType::Int];
        let mut ctx = SubstContext::new(0, &mut interner, &expr_types, &call_type_args, &index, &mut mangle_table);
        let result = ctx.substitute_expr(&call_expr, &HashMap::new());
        if let HirExpr::Call { callee, .. } = result {
            let name = ctx.interner.resolve(callee);
            assert!(name.contains("id"), "Callee should contain 'id', got {}", name);
            assert!(name.contains("i64"), "Callee should contain 'i64', got {}", name);
        } else { panic!("Expected Call"); }
        assert!(ctx.discovered.iter().any(|item| item.def_id == id_sym && item.kind == ItemKind::FnSpecialize));
        assert!(!ctx.output_expr_types.is_empty());
    }

    #[test]
    fn substitute_call_passthrough_non_generic() {
        let mut interner = Interner::new();
        let add_sym = interner.intern("add");
        let index = MonoIndex::build(&crate::Hir { items: vec![crate::HirItem::Fn(HirFn {
            doc: None, name: add_sym, type_params: vec![], params: vec![], param_mutability: vec![],
            ret: Some(HirType::Int), body: HirExpr::IntLit { id: ExprId::new(0), value: 0, span: Span::new(0,0) },
            span: Span::new(0,0), is_pub: false, is_macro_generated: false, is_extern_backed: false, is_test: false, test_config: None,
        })] });
        let mut mangle_table = MangleTable::new();
        let call_expr = HirExpr::Call { id: ExprId::new(0), callee: add_sym, args: vec![], span: Span::new(0,0) };
        let empty_map = HashMap::new();
        let mut ctx = SubstContext::new(0, &mut interner, &[HirType::Int], &empty_map, &index, &mut mangle_table);
        let empty_sub = HashMap::new();
        let result = ctx.substitute_expr(&call_expr, &empty_sub);
        if let HirExpr::Call { callee, .. } = result {
            assert_eq!(callee, add_sym);
        } else { panic!("Expected Call"); }
        assert!(ctx.discovered.is_empty());
    }

    #[test]
    fn substitute_as_concretizes_target_type() {
        let mut interner = Interner::new();
        let vec_sym = interner.intern("Vec");
        let t_sym = interner.intern("T");
        let index = MonoIndex::build(&crate::Hir { items: vec![crate::HirItem::Struct(StructDef { doc: None, name: vec_sym, type_params: vec![t_sym], fields: vec![StructField { name: interner.intern("data"), ty: HirType::Int, doc: None }], span: Span::new(0,0), is_pub: false })] });
        let mut mangle_table = MangleTable::new();
        let as_expr = HirExpr::As { id: ExprId::new(0), expr: Box::new(HirExpr::IntLit { id: ExprId::new(1), value: 42, span: Span::new(0,0) }), target_type: HirType::RawPtr(Box::new(HirType::Generic(vec_sym, vec![HirType::Named(t_sym)]))), span: Span::new(0,0) };
        let sub = HashMap::from([(t_sym, HirType::Int)]);
        let empty_map = HashMap::new();
        let mut ctx = SubstContext::new(0, &mut interner, &[HirType::Int, HirType::Int], &empty_map, &index, &mut mangle_table);
        let result = ctx.substitute_expr(&as_expr, &sub);
        if let HirExpr::As { target_type, .. } = result {
            match target_type { HirType::RawPtr(inner) => match inner.as_ref() { HirType::Named(sym) => { let name = interner.resolve(*sym); assert!(name.contains("Vec") && name.contains("i64")); } other => panic!("Expected Named inside RawPtr, got {:?}", other) }, other => panic!("Expected RawPtr, got {:?}", other) }
        } else { panic!("Expected As"); }
    }

    #[test]
    fn substitute_renumbers_expr_ids() {
        let mut interner = Interner::new();
        let index = MonoIndex::build(&crate::Hir { items: vec![] });
        let mut mangle_table = MangleTable::new();
        let expr = HirExpr::Binary { id: ExprId::new(0), op: crate::node::HirBinOp::Add, lhs: Box::new(HirExpr::IntLit { id: ExprId::new(1), value: 1, span: Span::new(0,0) }), rhs: Box::new(HirExpr::IntLit { id: ExprId::new(2), value: 2, span: Span::new(0,0) }), span: Span::new(0,0) };
        let empty_map = HashMap::new();
        let mut ctx = SubstContext::new(100, &mut interner, &[], &empty_map, &index, &mut mangle_table);
        let empty_sub = HashMap::new();
        let result = ctx.substitute_expr(&expr, &empty_sub);
        if let HirExpr::Binary { id, lhs, rhs, .. } = result {
            assert!(id.as_usize() >= 100);
            assert!(lhs.get_id().as_usize() >= 100);
            assert!(rhs.get_id().as_usize() >= 100);
        } else { panic!("Expected Binary"); }
        assert!(ctx.next_expr_id() >= 103);
    }

    #[test]
    fn substitute_sizeof_concretizes_target() {
        let mut interner = Interner::new();
        let vec_sym = interner.intern("Vec");
        let t_sym = interner.intern("T");
        let index = MonoIndex::build(&crate::Hir { items: vec![crate::HirItem::Struct(StructDef { doc: None, name: vec_sym, type_params: vec![t_sym], fields: vec![], span: Span::new(0,0), is_pub: false })] });
        let mut mangle_table = MangleTable::new();
        let expr = HirExpr::SizeOf { id: ExprId::new(0), target_type: HirType::Generic(vec_sym, vec![HirType::Named(t_sym)]), span: Span::new(0,0) };
        let sub = HashMap::from([(t_sym, HirType::Int)]);
        let empty_map = HashMap::new();
        let mut ctx = SubstContext::new(0, &mut interner, &[HirType::Int], &empty_map, &index, &mut mangle_table);
        let result = ctx.substitute_expr(&expr, &sub);
        if let HirExpr::SizeOf { target_type, .. } = result {
            match &target_type { HirType::Named(sym) => { let name = interner.resolve(*sym); assert!(name.contains("Vec") && name.contains("i64")); } other => panic!("Expected Named (mangled), got {:?}", other) }
        } else { panic!("Expected SizeOf"); }
    }
}
