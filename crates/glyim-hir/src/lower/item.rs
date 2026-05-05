use crate::HirFn;
use crate::item::{
    EnumDef, ExternBlock, ExternFn, HirImplDef, HirItem, HirVariant, StructDef, StructField,
};
use crate::lower::context::LoweringContext;
use crate::lower::expr::lower_expr;
use crate::lower::types::lower_type_expr;
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
            Some(HirItem::Fn(HirFn {
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
            }))
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
            let end = variants.last().map_or(name_span.end, |v| v.name_span.end);
            let hir_variants: Vec<HirVariant> = variants
                .iter()
                .enumerate()
                .map(|(i, v)| HirVariant {
                    name: v.name,
                    fields: match &v.kind {
                        glyim_parse::VariantKind::Unnamed(types)
                        | glyim_parse::VariantKind::Named(types) => types
                            .iter()
                            .map(|(sym, _, _)| StructField {
                                name: *sym,
                                ty: HirType::Int,
                                doc: None,
                            })
                            .collect(),
                    },
                    tag: i as u32,
                    doc: None,
                })
                .collect();
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
                        let mangled_name =
                            ctx.intern(&format!("{}_{}", ctx.resolve(*target), ctx.resolve(*name)));
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
                        Some(HirFn {
                            doc: None,
                            name: mangled_name,
                            type_params: all_tp,
                            params: hir_params,
                            param_mutability: mutabilities,
                            ret: ret.as_ref().map(|t| lower_type_expr(t, ctx)),
                            body: lower_expr(body, ctx),
                            span: glyim_diag::Span::new(span.start, body.span.end),
                            is_pub: false,
                            is_macro_generated: false,
                            is_extern_backed: false,
                        })
                    } else {
                        None
                    }
                })
                .collect();
            Some(HirItem::Impl(HirImplDef {
                doc: None,
                target_name: *target,
                type_params: type_params.clone(),
                methods: hir_methods,
                is_pub: *is_pub,
                span: *span,
            }))
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
            params: params.iter().map(|(sym, _span)| (*sym, HirType::Int)).collect(),
            param_mutability: params.iter().map(|_| false).collect(),
            ret: None,
            body: lower_expr(body, ctx),
            span: glyim_diag::Span::new(name_span.start, body.span.end),
            is_pub: false,
            is_macro_generated: true,
            is_extern_backed: false,
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
