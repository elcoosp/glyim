use crate::item::{
    EnumDef, ExternBlock, ExternFn, HirImplDef, HirItem, HirVariant, StructDef, StructField,
};
use crate::lower::context::LoweringContext;
use crate::lower::expr::lower_expr;
use crate::lower::types::lower_type_expr;
use crate::types::HirType;
use crate::HirFn;
use glyim_parse::Item;

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
                name: *name,
                type_params: vec![],
                params: vec![],
                ret: None,
                body: lower_expr(value, ctx),
                span: glyim_diag::Span::new(start, value.span.end),
                is_macro_generated: false,
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
            Some(HirItem::Fn(HirFn {
                name: *name,
                type_params: type_params.clone(),
                params: params
                    .iter()
                    .map(|(sym, _, ty)| {
                        (
                            *sym,
                            ty.as_ref()
                                .map(|t| lower_type_expr(t, ctx))
                                .unwrap_or(HirType::Int),
                        )
                    })
                    .collect(),
                ret: ret.as_ref().map(|t| lower_type_expr(t, ctx)),
                body: lower_expr(body, ctx),
                span: glyim_diag::Span::new(start, body.span.end),
                is_macro_generated: false,
            }))
        }
        Item::StructDef {
            name,
            name_span,
            fields,
            ..
        } => {
            let end = fields.last().map_or(name_span.end, |(_, s, _)| s.end);
            let hir_fields: Vec<StructField> = fields
                .iter()
                .map(|(sym, _, _)| StructField {
                    name: *sym,
                    ty: HirType::Int,
                })
                .collect();
            Some(HirItem::Struct(StructDef {
                name: *name,
                type_params: vec![],
                fields: hir_fields,
                span: glyim_diag::Span::new(name_span.start, end),
            }))
        }
        Item::EnumDef {
            name,
            name_span,
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
                            })
                            .collect(),
                    },
                    tag: i as u32,
                })
                .collect();
            Some(HirItem::Enum(EnumDef {
                name: *name,
                type_params: vec![],
                variants: hir_variants,
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
                        Some(HirFn {
                            name: mangled_name,
                            type_params: all_tp,
                            params: params
                                .iter()
                                .map(|(sym, _, ty)| {
                                    (
                                        *sym,
                                        ty.as_ref()
                                            .map(|t| lower_type_expr(t, ctx))
                                            .unwrap_or(HirType::Int),
                                    )
                                })
                                .collect(),
                            ret: ret.as_ref().map(|t| lower_type_expr(t, ctx)),
                            body: lower_expr(body, ctx),
                            span: glyim_diag::Span::new(span.start, body.span.end),
                            is_macro_generated: false,
                        })
                    } else {
                        None
                    }
                })
                .collect();
            Some(HirItem::Impl(HirImplDef {
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
            body,
            ..
        } => Some(HirItem::Fn(HirFn {
            name: *name,
            type_params: vec![],
            params: vec![],
            ret: None,
            body: lower_expr(body, ctx),
            span: glyim_diag::Span::new(name_span.start, body.span.end),
            is_macro_generated: true,
        })),
        Item::ExternBlock {
            span, functions, ..
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
                functions: ex_fns,
                span: *span,
            }))
        }
        Item::Use(_) | Item::Stmt(_) => None,
    }
}
