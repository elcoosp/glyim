//! Discovery of new specializations from types, calls, and expressions.
//!
//! This module answers: "Given this type/call/expression, what new
//! specializations need to be enqueued?" It produces `WorkItem`s
//! but does NOT process them — that's the BFS driver's job.
//!
//! Key principle: only enqueue specializations where ALL type arguments
//! are fully concrete (no unresolved type parameters). Partial
//! specializations are meaningless and would produce broken output.

use crate::monomorphize::concretize;
use crate::monomorphize::index::MonoIndex;
use crate::monomorphize::mangle_table::MangleTable;
use crate::monomorphize::work::WorkItem;
use crate::types::{ExprId, HirType};
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

/// Discover struct/enum specializations from a concrete type.
///
/// Walks the type tree and emits `StructSpecialize` / `EnumSpecialize`
/// work items for any `Generic`, `Option`, or `Result` types whose
/// arguments are fully concrete.
///
/// This is called BEFORE concretization so that nested generics like
/// `Generic("Vec", [Named("Option__i64")])` can still be discovered.
pub fn discover_type_specializations(
    ty: &HirType,
    index: &MonoIndex,
    interner: &mut Interner,
) -> Vec<WorkItem> {
    let mut items = Vec::new();
    discover_type_specializations_into(ty, index, interner, &mut items);
    items
}

fn discover_type_specializations_into(
    ty: &HirType,
    index: &MonoIndex,
    interner: &mut Interner,
    items: &mut Vec<WorkItem>,
) {
    match ty {
        HirType::Generic(sym, args) => {
            if args
                .iter()
                .any(|a| concretize::has_unresolved_type_param(a, interner))
            {
                for a in args {
                    discover_type_specializations_into(a, index, interner, items);
                }
                return;
            }

            if index.find_struct(*sym).is_some() {
                items.push(WorkItem::struct_specialize(*sym, args.clone()));
            } else if index.find_enum(*sym).is_some() {
                items.push(WorkItem::enum_specialize(*sym, args.clone()));
            }

            for a in args {
                discover_type_specializations_into(a, index, interner, items);
            }
        }

        HirType::Option(inner) => {
            if !concretize::has_unresolved_type_param(inner, interner) {
                let opt_sym = interner.intern("Option");
                if index.find_enum(opt_sym).is_some() {
                    items.push(WorkItem::enum_specialize(
                        opt_sym,
                        vec![inner.as_ref().clone()],
                    ));
                }
            }
            discover_type_specializations_into(inner, index, interner, items);
        }

        HirType::Result(ok, err) => {
            if !concretize::has_unresolved_type_param(ok, interner)
                && !concretize::has_unresolved_type_param(err, interner)
            {
                let res_sym = interner.intern("Result");
                if index.find_enum(res_sym).is_some() {
                    items.push(WorkItem::enum_specialize(
                        res_sym,
                        vec![ok.as_ref().clone(), err.as_ref().clone()],
                    ));
                }
            }
            discover_type_specializations_into(ok, index, interner, items);
            discover_type_specializations_into(err, index, interner, items);
        }

        HirType::RawPtr(inner) => {
            discover_type_specializations_into(inner, index, interner, items);
        }

        HirType::Tuple(elems) => {
            for e in elems {
                discover_type_specializations_into(e, index, interner, items);
            }
        }

        HirType::Func(params, ret) => {
            for p in params {
                discover_type_specializations_into(p, index, interner, items);
            }
            discover_type_specializations_into(ret, index, interner, items);
        }

        // Primitives, Named, Opaque, Never, Error — no discoveries needed
        _ => {}
    }
}

/// Discover function specializations from a Call expression.
///
/// Given the original ExprId of a call, looks up the typechecker's
/// `call_type_args` to find the concrete type arguments, applies
/// the current substitution, and enqueues a `FnSpecialize` work item.
///
/// Handles pre-mangled callee names (e.g., `Vec_push__i64`) by
/// demangling them to find the base generic function name.
///
/// Returns:
/// - The mangled callee name to use in the output HIR
/// - Any work items discovered
pub fn discover_call_specialization(
    original_id: ExprId,
    callee: Symbol,
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
    sub: &HashMap<Symbol, HirType>,
    index: &MonoIndex,
    mangle_table: &mut MangleTable,
    interner: &mut Interner,
) -> (Symbol, Vec<WorkItem>) {
    let mut items = Vec::new();

    // Step 1: Look up type args from the typechecker.
    let raw_type_args = match call_type_args.get(&original_id) {
        Some(args) => args.clone(),
        None => return (callee, items),
    };

    // Step 2: Apply substitution to type args, then concretize.
    let concrete_type_args: Vec<HirType> = raw_type_args
        .iter()
        .map(|t| concretize::substitute_and_concretize(t, sub, index, mangle_table, interner))
        .collect();

    // Step 3: Check that all type args are fully concrete.
    if concrete_type_args.is_empty()
        || concrete_type_args
            .iter()
            .any(|t| concretize::has_unresolved_type_param(t, interner))
    {
        return (callee, items);
    }

    // Step 4: Demangle the callee name if it's pre-mangled.
    let callee_str = interner.resolve(callee).to_string();
    let base_callee = if let Some(pos) = callee_str.find("__") {
        let base_str = &callee_str[..pos];
        interner.intern(base_str)
    } else {
        callee
    };

    // Step 5: Only enqueue specialization if the function is actually generic.
    if index.is_generic_fn(base_callee) {
        items.push(WorkItem::fn_specialize(
            base_callee,
            concrete_type_args.clone(),
        ));
    }

    // Step 6: Compute the mangled callee name for the output HIR.
    let new_callee = mangle_table.mangle_fn(base_callee, &concrete_type_args, interner);

    (new_callee, items)
}

/// Discover function specializations from a MethodCall expression.
///
/// Uses the receiver's type to determine the base method name
/// (e.g., `Vec_push`), then follows the same logic as call discovery.
///
/// Returns:
/// - `Some((mangled_callee, work_items))` if the method call can be
///   desugared to a regular call
/// - `None` if the method call cannot be resolved (kept as MethodCall
///   in the output)
pub fn discover_method_call_specialization(
    original_id: ExprId,
    receiver_original_id: ExprId,
    method_name: Symbol,
    input_expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
    sub: &HashMap<Symbol, HirType>,
    index: &MonoIndex,
    mangle_table: &mut MangleTable,
    interner: &mut Interner,
) -> Option<(Symbol, Vec<WorkItem>)> {
    let mut items = Vec::new();

    let raw_type_args = call_type_args.get(&original_id).cloned();

    let receiver_ty = input_expr_types
        .get(receiver_original_id.as_usize())
        .cloned();

    let inner_ty = match &receiver_ty {
        Some(HirType::RawPtr(inner)) => inner.as_ref().clone(),
        Some(other) => other.clone(),
        None => return None,
    };

    let type_name = match &inner_ty {
        HirType::Named(name) => *name,
        HirType::Generic(name, _) => *name,
        _ => return None,
    };

    let type_str = interner.resolve(type_name);
    let method_str = interner.resolve(method_name);
    let base_method_name = interner.intern(&format!("{}_{}", type_str, method_str));

    let concrete_type_args: Vec<HirType> = if let Some(raw_args) = raw_type_args {
        raw_args
            .iter()
            .map(|t| concretize::substitute_and_concretize(t, sub, index, mangle_table, interner))
            .collect()
    } else {
        match &inner_ty {
            HirType::Generic(_, type_args) => type_args
                .iter()
                .map(|t| {
                    concretize::substitute_and_concretize(t, sub, index, mangle_table, interner)
                })
                .collect(),
            _ => return None,
        }
    };

    if concrete_type_args.is_empty()
        || concrete_type_args
            .iter()
            .any(|t| concretize::has_unresolved_type_param(t, interner))
    {
        return None;
    }

    if index.is_generic_fn(base_method_name) {
        items.push(WorkItem::fn_specialize(
            base_method_name,
            concrete_type_args.clone(),
        ));
    }

    let new_callee = mangle_table.mangle_fn(base_method_name, &concrete_type_args, interner);

    Some((new_callee, items))
}

/// Discover iter() and next() specializations from a ForIn expression.
pub fn discover_forin_specializations(
    iter_original_id: ExprId,
    input_expr_types: &[HirType],
    sub: &HashMap<Symbol, HirType>,
    index: &MonoIndex,
    mangle_table: &mut MangleTable,
    interner: &mut Interner,
) -> Vec<WorkItem> {
    let mut items = Vec::new();

    let iter_ty = match input_expr_types.get(iter_original_id.as_usize()) {
        Some(ty) => ty.clone(),
        None => return items,
    };

    let inner_ty = match &iter_ty {
        HirType::RawPtr(inner) => inner.as_ref().clone(),
        other => other.clone(),
    };

    if let HirType::Generic(type_name, type_args) = &inner_ty {
        let concrete_args: Vec<HirType> = type_args
            .iter()
            .map(|a| concretize::substitute_and_concretize(a, sub, index, mangle_table, interner))
            .collect();

        if concrete_args.is_empty()
            || concrete_args
                .iter()
                .any(|a| concretize::has_unresolved_type_param(a, interner))
        {
            return items;
        }

        let iter_method = interner.intern(&format!("{}_iter", interner.resolve(*type_name)));

        if let Some(iter_fn) = index.find_fn(iter_method) {
            items.push(WorkItem::fn_specialize(iter_method, concrete_args.clone()));

            let iter_sub = concretize::build_subst(&iter_fn.type_params, &concrete_args);
            if let Some(ret) = &iter_fn.ret {
                let ret_ty = crate::types::substitute_type(ret, &iter_sub);
                if let HirType::Generic(iter_name, iter_args) = &ret_ty {
                    let next_method =
                        interner.intern(&format!("{}_next", interner.resolve(*iter_name)));
                    if index.find_fn(next_method).is_some() {
                        let next_concrete: Vec<HirType> = iter_args
                            .iter()
                            .map(|a| {
                                concretize::substitute_and_concretize(
                                    a,
                                    &iter_sub,
                                    index,
                                    mangle_table,
                                    interner,
                                )
                            })
                            .collect();
                        if !next_concrete.is_empty()
                            && next_concrete
                                .iter()
                                .all(|a| !concretize::has_unresolved_type_param(a, interner))
                        {
                            items.push(WorkItem::fn_specialize(next_method, next_concrete));
                        }
                    }
                }
            }
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{EnumDef, StructDef, StructField};
    use crate::monomorphize::index::MonoIndex;
    use crate::monomorphize::mangle_table::MangleTable;
    use crate::monomorphize::work::ItemKind;
    use crate::node::HirFn;
    use crate::types::ExprId;
    use crate::types::HirType;
    use glyim_diag::Span;
    use glyim_interner::Interner;

    fn build_index_with_vec(interner: &mut Interner) -> MonoIndex {
        let vec_sym = interner.intern("Vec");
        let t_sym = interner.intern("T");

        let hir = crate::Hir {
            items: vec![
                crate::HirItem::Struct(StructDef {
                    doc: None,
                    name: vec_sym,
                    type_params: vec![t_sym],
                    fields: vec![StructField {
                        name: interner.intern("data"),
                        ty: HirType::Int,
                        doc: None,
                    }],
                    span: Span::new(0, 0),
                    is_pub: false,
                }),
                crate::HirItem::Enum(EnumDef {
                    doc: None,
                    name: interner.intern("Option"),
                    type_params: vec![t_sym],
                    variants: vec![],
                    span: Span::new(0, 0),
                    is_pub: false,
                }),
            ],
        };

        MonoIndex::build(&hir)
    }

    #[test]
    fn discover_type_specializations_generic_struct() {
        let mut interner = Interner::new();
        let vec_sym = interner.intern("Vec");
        let index = build_index_with_vec(&mut interner);

        let ty = HirType::Generic(vec_sym, vec![HirType::Int]);
        let items = discover_type_specializations(&ty, &index, &mut interner);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ItemKind::StructSpecialize);
        assert_eq!(items[0].def_id, vec_sym);
        assert_eq!(items[0].type_args, vec![HirType::Int]);
    }

    #[test]
    fn discover_type_specializations_unresolved_param_skipped() {
        let mut interner = Interner::new();
        let vec_sym = interner.intern("Vec");
        let t_sym = interner.intern("T");
        let index = build_index_with_vec(&mut interner);

        let ty = HirType::Generic(vec_sym, vec![HirType::Named(t_sym)]);
        let items = discover_type_specializations(&ty, &index, &mut interner);

        assert!(
            items.is_empty(),
            "Should not specialize with unresolved type param"
        );
    }

    #[test]
    fn discover_type_specializations_option() {
        let mut interner = Interner::new();
        let opt_sym = interner.intern("Option");
        let index = build_index_with_vec(&mut interner);

        let ty = HirType::Option(Box::new(HirType::Int));
        let items = discover_type_specializations(&ty, &index, &mut interner);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ItemKind::EnumSpecialize);
        assert_eq!(items[0].def_id, opt_sym);
    }

    #[test]
    fn discover_type_specializations_raw_ptr_generic() {
        let mut interner = Interner::new();
        let vec_sym = interner.intern("Vec");
        let index = build_index_with_vec(&mut interner);

        let ty = HirType::RawPtr(Box::new(HirType::Generic(vec_sym, vec![HirType::Int])));
        let items = discover_type_specializations(&ty, &index, &mut interner);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ItemKind::StructSpecialize);
    }

    #[test]
    fn discover_call_specialization_generic_fn() {
        let mut interner = Interner::new();
        let id_sym = interner.intern("id");
        let t_sym = interner.intern("T");

        let hir = crate::Hir {
            items: vec![crate::HirItem::Fn(HirFn {
                doc: None,
                name: id_sym,
                type_params: vec![t_sym],
                params: vec![(interner.intern("x"), HirType::Named(t_sym))],
                param_mutability: vec![false],
                ret: Some(HirType::Named(t_sym)),
                body: crate::node::HirExpr::IntLit {
                    id: crate::types::ExprId::new(0),
                    value: 0,
                    span: Span::new(0, 0),
                },
                span: Span::new(0, 0),
                is_pub: false,
                is_macro_generated: false,
                is_extern_backed: false,
                is_test: false,
                test_config: None,
            })],
        };
        let index = MonoIndex::build(&hir);
        let mut mangle_table = MangleTable::new();

        let call_type_args = HashMap::from([(ExprId::new(42), vec![HirType::Int])]);
        let sub = HashMap::new();

        let (new_callee, items) = discover_call_specialization(
            ExprId::new(42),
            id_sym,
            &call_type_args,
            &sub,
            &index,
            &mut mangle_table,
            &mut interner,
        );

        let name = interner.resolve(new_callee);
        assert!(
            name.contains("id"),
            "Callee should contain 'id', got {}",
            name
        );
        assert!(
            name.contains("i64"),
            "Callee should contain 'i64', got {}",
            name
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ItemKind::FnSpecialize);
        assert_eq!(items[0].def_id, id_sym);
        assert_eq!(items[0].type_args, vec![HirType::Int]);
    }

    #[test]
    fn discover_call_specialization_non_generic_fn() {
        let mut interner = Interner::new();
        let add_sym = interner.intern("add");

        let hir = crate::Hir {
            items: vec![crate::HirItem::Fn(HirFn {
                doc: None,
                name: add_sym,
                type_params: vec![],
                params: vec![],
                param_mutability: vec![],
                ret: Some(HirType::Int),
                body: crate::node::HirExpr::IntLit {
                    id: crate::types::ExprId::new(0),
                    value: 0,
                    span: Span::new(0, 0),
                },
                span: Span::new(0, 0),
                is_pub: false,
                is_macro_generated: false,
                is_extern_backed: false,
                is_test: false,
                test_config: None,
            })],
        };
        let index = MonoIndex::build(&hir);
        let mut mangle_table = MangleTable::new();

        let call_type_args = HashMap::new();
        let sub = HashMap::new();

        let (new_callee, items) = discover_call_specialization(
            ExprId::new(42),
            add_sym,
            &call_type_args,
            &sub,
            &index,
            &mut mangle_table,
            &mut interner,
        );

        assert_eq!(new_callee, add_sym);
        assert!(items.is_empty());
    }

    #[test]
    fn discover_call_specialization_demangles_pre_mangled() {
        let mut interner = Interner::new();
        let mangled_callee = interner.intern("Vec_push__i64");
        let base_push = interner.intern("Vec_push");
        let t_sym = interner.intern("T");

        let hir = crate::Hir {
            items: vec![crate::HirItem::Fn(HirFn {
                doc: None,
                name: base_push,
                type_params: vec![t_sym],
                params: vec![],
                param_mutability: vec![],
                ret: None,
                body: crate::node::HirExpr::IntLit {
                    id: crate::types::ExprId::new(0),
                    value: 0,
                    span: Span::new(0, 0),
                },
                span: Span::new(0, 0),
                is_pub: false,
                is_macro_generated: false,
                is_extern_backed: false,
                is_test: false,
                test_config: None,
            })],
        };
        let index = MonoIndex::build(&hir);
        let mut mangle_table = MangleTable::new();

        let call_type_args = HashMap::from([(ExprId::new(42), vec![HirType::Int])]);
        let sub = HashMap::new();

        let (new_callee, items) = discover_call_specialization(
            ExprId::new(42),
            mangled_callee,
            &call_type_args,
            &sub,
            &index,
            &mut mangle_table,
            &mut interner,
        );

        assert_eq!(items.len(), 1, "Should discover Vec_push specialization");
        assert_eq!(items[0].def_id, base_push, "Should use demangled base name");
    }
}

/// After a function has been substituted, walk the concrete body to find
/// generic calls that still need specialisation (e.g. Iter_new inside Vec_iter).
///
/// IMPORTANT: Only enqueues work items when concrete type arguments can be
/// inferred. Never enqueues FnSpecialize with empty type_args — that would
/// produce functions with unresolved type parameters.
pub fn discover_calls_in_body(
    expr: &crate::node::HirExpr,
    index: &MonoIndex,
    interner: &mut Interner,
    mangle_table: &mut MangleTable,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
    sub: &HashMap<Symbol, HirType>,
) -> Vec<WorkItem> {
    let mut items = Vec::new();
    discover_calls_in_expr(
        expr,
        index,
        interner,
        mangle_table,
        expr_types,
        call_type_args,
        sub,
        &mut items,
    );
    items
}

fn discover_calls_in_expr(
    expr: &crate::node::HirExpr,
    index: &MonoIndex,
    interner: &mut Interner,
    mangle_table: &mut MangleTable,
    expr_types: &[HirType],
    call_type_args: &HashMap<ExprId, Vec<HirType>>,
    sub: &HashMap<Symbol, HirType>,
    items: &mut Vec<WorkItem>,
) {
    use crate::node::{HirExpr, HirStmt};
    match expr {
        HirExpr::Call {
            id, callee, args, ..
        } => {
            let callee_str = interner.resolve(*callee).to_string();
            let base = if let Some(pos) = callee_str.find("__") {
                interner.intern(&callee_str[..pos])
            } else {
                *callee
            };
            if index.is_generic_fn(base) {
                // Try call_type_args first (most reliable source)
                if let Some(type_args) = call_type_args.get(id) {
                    let concrete_args: Vec<HirType> = type_args
                        .iter()
                        .map(|t| {
                            concretize::substitute_and_concretize(
                                t,
                                sub,
                                index,
                                mangle_table,
                                interner,
                            )
                        })
                        .collect();
                    let all_concrete = concrete_args
                        .iter()
                        .all(|a| !concretize::has_unresolved_type_param(a, interner));
                    if !concrete_args.is_empty() && all_concrete {
                        items.push(WorkItem::fn_specialize(base, concrete_args));
                    }
                } else {
                    // Try inferring from the call's return type
                    if let Some(return_ty) = expr_types.get(id.as_usize()) {
                        if let HirType::Generic(_, type_args) = return_ty {
                            let concrete_args: Vec<HirType> = type_args
                                .iter()
                                .map(|a| {
                                    concretize::substitute_and_concretize(
                                        a,
                                        sub,
                                        index,
                                        mangle_table,
                                        interner,
                                    )
                                })
                                .collect();
                            if !concrete_args.is_empty()
                                && concrete_args
                                    .iter()
                                    .all(|a| !concretize::has_unresolved_type_param(a, interner))
                            {
                                items.push(WorkItem::fn_specialize(base, concrete_args));
                            }
                        }
                    }
                }
            }
            for a in args {
                discover_calls_in_expr(
                    a,
                    index,
                    interner,
                    mangle_table,
                    expr_types,
                    call_type_args,
                    sub,
                    items,
                );
            }
        }
        HirExpr::Block { stmts, .. } => {
            for s in stmts {
                match s {
                    HirStmt::Expr(e)
                    | HirStmt::Let { value: e, .. }
                    | HirStmt::LetPat { value: e, .. }
                    | HirStmt::Assign { value: e, .. }
                    | HirStmt::AssignField { value: e, .. }
                    | HirStmt::AssignDeref { value: e, .. } => {
                        discover_calls_in_expr(
                            e,
                            index,
                            interner,
                            mangle_table,
                            expr_types,
                            call_type_args,
                            sub,
                            items,
                        );
                    } // All HirStmt variants are covered above; no unreachable wildcard needed
                }
            }
        }
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            discover_calls_in_expr(
                condition,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
            discover_calls_in_expr(
                then_branch,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
            if let Some(e) = else_branch {
                discover_calls_in_expr(
                    e,
                    index,
                    interner,
                    mangle_table,
                    expr_types,
                    call_type_args,
                    sub,
                    items,
                );
            }
        }
        HirExpr::While {
            condition, body, ..
        } => {
            discover_calls_in_expr(
                condition,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
            discover_calls_in_expr(
                body,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
        }
        HirExpr::ForIn { iter, body, .. } => {
            // Discover iterator/next specializations
            let forin_items = discover_forin_specializations(
                iter.get_id(),
                expr_types,
                sub,
                index,
                mangle_table,
                interner,
            );
            items.extend(forin_items);
            // Also discover type specializations from the iterator's type
            if let Some(iter_ty) = expr_types.get(iter.get_id().as_usize()) {
                discover_type_specializations_into(iter_ty, index, interner, items);
            }
            discover_calls_in_expr(
                iter,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
            discover_calls_in_expr(
                body,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            discover_calls_in_expr(
                scrutinee,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
            for arm in arms {
                if let Some(ref g) = arm.guard {
                    discover_calls_in_expr(
                        g,
                        index,
                        interner,
                        mangle_table,
                        expr_types,
                        call_type_args,
                        sub,
                        items,
                    );
                }
                discover_calls_in_expr(
                    &arm.body,
                    index,
                    interner,
                    mangle_table,
                    expr_types,
                    call_type_args,
                    sub,
                    items,
                );
            }
        }
        HirExpr::Return { value: Some(v), .. } => {
            discover_calls_in_expr(
                v,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
        }
        HirExpr::Return { value: None, .. } => {}
        HirExpr::MethodCall { receiver, args, .. } => {
            discover_calls_in_expr(
                receiver,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
            for a in args {
                discover_calls_in_expr(
                    a,
                    index,
                    interner,
                    mangle_table,
                    expr_types,
                    call_type_args,
                    sub,
                    items,
                );
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            discover_calls_in_expr(
                lhs,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
            discover_calls_in_expr(
                rhs,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
        }
        HirExpr::Unary { operand, .. } | HirExpr::Deref { expr: operand, .. } => {
            discover_calls_in_expr(
                operand,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
        }
        HirExpr::StructLit {
            id,
            struct_name: _,
            fields,
            ..
        } => {
            // Discover struct specialization from the expression's inferred type
            if let Some(expr_ty) = expr_types.get(id.as_usize()) {
                discover_type_specializations_into(expr_ty, index, interner, items);
            }
            for (_, val) in fields {
                discover_calls_in_expr(
                    val,
                    index,
                    interner,
                    mangle_table,
                    expr_types,
                    call_type_args,
                    sub,
                    items,
                );
            }
        }
        HirExpr::EnumVariant {
            id,
            enum_name: _,
            variant_name: _,
            args,
            ..
        } => {
            // Discover enum specialization from the expression's inferred type
            if let Some(expr_ty) = expr_types.get(id.as_usize()) {
                discover_type_specializations_into(expr_ty, index, interner, items);
            }
            for a in args {
                discover_calls_in_expr(
                    a,
                    index,
                    interner,
                    mangle_table,
                    expr_types,
                    call_type_args,
                    sub,
                    items,
                );
            }
        }
        HirExpr::TupleLit { elements, .. } => {
            for a in elements {
                discover_calls_in_expr(
                    a,
                    index,
                    interner,
                    mangle_table,
                    expr_types,
                    call_type_args,
                    sub,
                    items,
                );
            }
        }
        HirExpr::Println { arg, .. } => {
            discover_calls_in_expr(
                arg,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
        }
        HirExpr::Assert {
            condition, message, ..
        } => {
            discover_calls_in_expr(
                condition,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
            if let Some(m) = message {
                discover_calls_in_expr(
                    m,
                    index,
                    interner,
                    mangle_table,
                    expr_types,
                    call_type_args,
                    sub,
                    items,
                );
            }
        }
        HirExpr::As { expr: inner, .. } => {
            discover_calls_in_expr(
                inner,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
        }
        HirExpr::FieldAccess { object, .. } => {
            discover_calls_in_expr(
                object,
                index,
                interner,
                mangle_table,
                expr_types,
                call_type_args,
                sub,
                items,
            );
        }
        HirExpr::SizeOf {
            id, target_type, ..
        } => {
            // Discover type specializations from the target type.
            // The type checker records the target_type in call_type_args for SizeOf.
            if let Some(type_args) = call_type_args.get(id) {
                for ta in type_args {
                    discover_type_specializations_into(ta, index, interner, items);
                }
            }
            // Also discover from the target_type directly (in case call_type_args is empty)
            discover_type_specializations_into(target_type, index, interner, items);
        }
        // Leaf nodes: IntLit, FloatLit, BoolLit, StrLit, UnitLit, Ident, AddrOf
        _ => {}
    }
}
