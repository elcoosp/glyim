use crate::lower::context::LoweringContext;
use crate::HirPattern;
use glyim_parse::Pattern;

pub fn lower_pattern(pat: &Pattern, ctx: &mut LoweringContext) -> HirPattern {
    lower_pattern_with_span(pat, None, ctx)
}

fn lower_pattern_with_span(
    pat: &Pattern,
    span: Option<glyim_diag::Span>,
    ctx: &mut LoweringContext,
) -> HirPattern {
    match pat {
        Pattern::Wild => HirPattern::Wild,
        Pattern::BoolLit(b) => HirPattern::BoolLit(*b),
        Pattern::IntLit(n) => HirPattern::IntLit(*n),
        Pattern::FloatLit(f) => HirPattern::FloatLit(*f),
        Pattern::StrLit(s) => HirPattern::StrLit(s.clone()),
        Pattern::Unit => HirPattern::Unit,
        Pattern::Var(sym) => HirPattern::Var(*sym),
        Pattern::Struct { name, fields } => HirPattern::Struct {
            name: *name,
            bindings: fields
                .iter()
                .map(|(sym, p)| (*sym, lower_pattern_with_span(p, None, ctx)))
                .collect(),
            span: span.unwrap_or(glyim_diag::Span::new(0, 0)),
        },
        Pattern::EnumVariant {
            enum_name,
            variant_name,
            args,
        } => HirPattern::EnumVariant {
            enum_name: *enum_name,
            variant_name: *variant_name,
            bindings: args
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    (
                        ctx.intern(&i.to_string()),
                        lower_pattern_with_span(p, None, ctx),
                    )
                })
                .collect(),
            span: span.unwrap_or(glyim_diag::Span::new(0, 0)),
        },
        Pattern::Tuple(elems) => HirPattern::Tuple {
            elements: elems
                .iter()
                .map(|e| lower_pattern_with_span(e, None, ctx))
                .collect(),
            span: span.unwrap_or(glyim_diag::Span::new(0, 0)),
        },
        Pattern::OptionSome(p) => {
            HirPattern::OptionSome(Box::new(lower_pattern_with_span(p, None, ctx)))
        }
        Pattern::OptionNone => HirPattern::OptionNone,
        Pattern::ResultOk(p) => {
            HirPattern::ResultOk(Box::new(lower_pattern_with_span(p, None, ctx)))
        }
        Pattern::ResultErr(p) => {
            HirPattern::ResultErr(Box::new(lower_pattern_with_span(p, None, ctx)))
        }
    }
}
