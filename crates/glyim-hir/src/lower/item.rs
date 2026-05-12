use crate::HirFn;
use crate::item::{
    EnumDef, ExternBlock, ExternFn, HirImplDef, HirItem, HirVariant, StructDef, StructField,
};
use crate::lower::context::LoweringContext;
use crate::lower::expr::lower_expr;
use crate::lower::types::lower_type_expr;
use crate::node::HirTestConfig;
use crate::types::HirType;
use glyim_parse::Item;

#[allow(dead_code)]
/// Check the declaration table for a type name, returning true if it
/// is known to be a struct (as opposed to an enum or unknown).
fn is_known_struct(ctx: &LoweringContext, name: glyim_interner::Symbol) -> bool {
    ctx.struct_names.contains(&name)
        || ctx
            .decl_table
            .and_then(|dt| dt.structs.get(&name))
            .is_some()
}

/// Replace HirType::Named symbols that match type parameters with HirType::Param.
fn replace_named_with_param(ty: HirType, type_params: &[glyim_interner::Symbol]) -> HirType {
    if type_params.is_empty() {
        return ty;
    }
    match ty {
        HirType::Named(sym) if type_params.contains(&sym) => HirType::Param(sym),
        HirType::Generic(base, args) => HirType::Generic(
            base,
            args.into_iter()
                .map(|a| replace_named_with_param(a, type_params))
                .collect(),
        ),
        HirType::RawPtr(inner) => {
            HirType::RawPtr(Box::new(replace_named_with_param(*inner, type_params)))
        }
        HirType::Tuple(elems) => HirType::Tuple(
            elems
                .into_iter()
                .map(|e| replace_named_with_param(e, type_params))
                .collect(),
        ),
        HirType::Func(params, ret) => HirType::Func(
            params
                .into_iter()
                .map(|p| replace_named_with_param(p, type_params))
                .collect(),
            Box::new(replace_named_with_param(*ret, type_params)),
        ),
        other => other,
    }
}

pub fn lower_item(item: &Item, ctx: &mut LoweringContext) -> Option<HirItem> {
    match item {
        Item::Binding {
            name,
            name_span,
            value,
            attrs,
            ..
        } => {
            let start = attrs.first().map_or(name_span.start, |a| a.span.start);
            Some(HirItem::Fn(HirFn {
                doc: None,
                name: *name,
                type_params: vec![],
                params: vec![],
                param_mutability: vec![],
                ret: None,
                body: lower_expr(value, ctx),
                span: glyim_diag::Span::new(start, value.span.end),
                is_pub: false,
                is_macro_generated: false,
                is_extern_backed: false,
                is_test: false,
                test_config: None,
            }))
        }
        Item::FnDef {
            name,
            name_span,
            type_params,
            params,
            ret,
            body,
            attrs,
            ..
        } => {
            let start = attrs.first().map_or(name_span.start, |a| a.span.start);
            ctx.push_type_params(type_params);
            let (hir_params, mutabilities): (Vec<_>, Vec<_>) = params
                .iter()
                .map(|(sym, _, ty, mutable)| {
                    (
                        (
                            *sym,
                            ty.as_ref()
                                .map(|t| lower_type_expr(t, ctx))
                                .unwrap_or(HirType::Int),
                        ),
                        *mutable,
                    )
                })
                .unzip();
            let is_test = attrs.iter().any(|a| a.name == "test");
            let should_panic = attrs
                .iter()
                .any(|a| a.name == "test" && a.args.iter().any(|arg| arg.key == "should_panic"));
            let ignored = attrs.iter().any(|a| a.name == "ignore");
            let test_config = if is_test {
                Some(HirTestConfig {
                    should_panic,
                    ignored,
                    tags: Vec::new(),
                    source_file: String::new(),
                })
            } else {
                None
            };
            let hir_item = HirItem::Fn(HirFn {
                doc: None,
                name: *name,
                type_params: type_params.clone(),
                params: hir_params,
                param_mutability: mutabilities,
                ret: ret.as_ref().map(|t| lower_type_expr(t, ctx)),
                body: lower_expr(body, ctx),
                span: glyim_diag::Span::new(start, body.span.end),
                is_pub: false,
                is_macro_generated: false,
                is_extern_backed: false,
                is_test,
                test_config,
            });
            ctx.pop_type_params();
            Some(hir_item)
        }
        Item::StructDef {
            doc: _,
            name,
            name_span,
            type_params,
            fields,
            ..
        } => {
            // Register this struct's name so we can later recognise
            // calls like Point::zero() as struct associated functions.
            ctx.struct_names.insert(*name);
            ctx.push_type_params(type_params);
            let end = fields.last().map_or(name_span.end, |(_, s, _)| s.end);
            let hir_fields: Vec<StructField> = fields
                .iter()
                .map(|(sym, _, ty)| StructField {
                    name: *sym,
                    ty: ty
                        .as_ref()
                        .map(|t| lower_type_expr(t, ctx))
                        .unwrap_or(HirType::Int),
                    doc: None,
                })
                .collect();
            ctx.pop_type_params();
            Some(HirItem::Struct(StructDef {
                doc: None,
                name: *name,
                type_params: type_params.clone(),
                fields: hir_fields,
                is_pub: false,
                span: glyim_diag::Span::new(name_span.start, end),
            }))
        }
        Item::EnumDef {
            doc: _,
            name,
            name_span,
            type_params,
            variants,
            ..
        } => {
            ctx.push_type_params(type_params);
            let end = variants.last().map_or(name_span.end, |v| v.name_span.end);
            let hir_variants: Vec<HirVariant> = variants
                .iter()
                .enumerate()
                .map(|(i, v)| HirVariant {
                    name: v.name,
                    fields: match &v.kind {
                        glyim_parse::VariantKind::Unnamed(types) => types
                            .iter()
                            .enumerate()
                            .map(|(j, (sym, _, ty_opt))| {
                                // For unnamed variants, the field symbol is the type expression.
                                // The field name is synthesized (e.g., __0, __1).
                                let field_name = ctx.intern(&format!("__{}", j));
                                let ty = ty_opt
                                    .as_ref()
                                    .map(|t| crate::lower::types::lower_type_expr(t, ctx))
                                    .unwrap_or_else(|| {
                                        // Interpret the field symbol as a type expression
                                        let type_expr = glyim_parse::TypeExpr::Named(*sym);
                                        crate::lower::types::lower_type_expr(&type_expr, ctx)
                                    });
                                StructField {
                                    name: field_name,
                                    ty,
                                    doc: None,
                                }
                            })
                            .collect(),
                        glyim_parse::VariantKind::Named(types) => types
                            .iter()
                            .map(|(sym, _, ty_opt)| {
                                // For named variants, field name is explicit, type from annotation or Int
                                let ty = ty_opt
                                    .as_ref()
                                    .map(|t| crate::lower::types::lower_type_expr(t, ctx))
                                    .unwrap_or(HirType::Int);
                                StructField {
                                    name: *sym,
                                    ty,
                                    doc: None,
                                }
                            })
                            .collect(),
                    },
                    tag: i as u32,
                    doc: None,
                })
                .collect();
            ctx.pop_type_params();
            Some(HirItem::Enum(EnumDef {
                doc: None,
                name: *name,
                type_params: type_params.clone(),
                variants: hir_variants,
                is_pub: false,
                span: glyim_diag::Span::new(name_span.start, end),
            }))
        }
        Item::ImplBlock {
            target,
            type_params,
            methods,
            is_pub,
            span,
            ..
        } => {
            let hir_methods: Vec<HirFn> = methods
                .iter()
                .filter_map(|m| {
                    if let Item::FnDef {
                        name,
                        type_params: fn_tp,
                        params,
                        ret,
                        body,
                        ..
                    } = m
                    {
                        let all_tp: Vec<_> =
                            type_params.iter().chain(fn_tp.iter()).copied().collect();
                        ctx.push_type_params(&all_tp);
                        let mangled_name =
                            ctx.intern(&format!("{}_{}", ctx.resolve(*target), ctx.resolve(*name)));
                        let combined_type_params: Vec<_> =
                            type_params.iter().chain(fn_tp.iter()).copied().collect();
                        let (hir_params, mutabilities): (Vec<_>, Vec<_>) = params
                            .iter()
                            .map(|(sym, _, ty, mutable)| {
                                let raw_ty = ty
                                    .as_ref()
                                    .map(|t| lower_type_expr(t, ctx))
                                    .unwrap_or(HirType::Int);
                                // Convert Named type params to HirType::Param
                                let resolved_ty =
                                    replace_named_with_param(raw_ty, &combined_type_params);
                                ((*sym, resolved_ty), *mutable)
                            })
                            .unzip();
                        let method = HirFn {
                            doc: None,
                            name: mangled_name,
                            type_params: all_tp.clone(),
                            params: hir_params,
                            param_mutability: mutabilities,
                            ret: ret.as_ref().map(|t| {
                                let raw_ret = lower_type_expr(t, ctx);
                                replace_named_with_param(raw_ret, &combined_type_params)
                            }),
                            body: lower_expr(body, ctx),
                            span: glyim_diag::Span::new(span.start, body.span.end),
                            is_pub: false,
                            is_macro_generated: false,
                            is_extern_backed: false,
                            is_test: false,
                            test_config: None,
                        };
                        ctx.pop_type_params();
                        Some(method)
                    } else {
                        None
                    }
                })
                .collect();
            let result = Some(HirItem::Impl(HirImplDef {
                doc: None,
                target_name: *target,
                type_params: type_params.clone(),
                methods: hir_methods,
                is_pub: *is_pub,
                span: *span,
            }));
            ctx.pop_type_params();
            result
        }
        Item::MacroDef {
            name,
            name_span,
            params,
            body,
            ..
        } => Some(HirItem::Fn(HirFn {
            doc: None,
            name: *name,
            type_params: vec![],
            params: params
                .iter()
                .map(|(sym, _span)| (*sym, HirType::Int))
                .collect(),
            param_mutability: params.iter().map(|_| false).collect(),
            ret: None,
            body: lower_expr(body, ctx),
            span: glyim_diag::Span::new(name_span.start, body.span.end),
            is_pub: false,
            is_macro_generated: true,
            is_extern_backed: false,
            is_test: false,
            test_config: None,
        })),
        Item::ExternBlock {
            doc: _,
            span,
            functions,
            ..
        } => {
            let ex_fns: Vec<ExternFn> = functions
                .iter()
                .map(|f| ExternFn {
                    name: f.name,
                    params: f
                        .params
                        .iter()
                        .map(|(_, _, ty)| {
                            ty.as_ref()
                                .map(|t| lower_type_expr(t, ctx))
                                .unwrap_or(HirType::Int)
                        })
                        .collect(),
                    ret: f
                        .ret
                        .as_ref()
                        .map(|t| lower_type_expr(t, ctx))
                        .unwrap_or(HirType::Int),
                })
                .collect();
            Some(HirItem::Extern(ExternBlock {
                doc: None,
                functions: ex_fns,
                span: *span,
            }))
        }
        Item::Use(_) | Item::Stmt(_) => None,
    }
}
