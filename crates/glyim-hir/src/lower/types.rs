use crate::lower::context::LoweringContext;
use crate::types::HirType;
use glyim_parse::TypeExpr;

#[allow(clippy::only_used_in_recursion)]
pub fn lower_type_expr(ty: &TypeExpr, ctx: &mut LoweringContext) -> HirType {
    match ty {
        TypeExpr::Int => HirType::Int,
        TypeExpr::Float => HirType::Float,
        TypeExpr::Bool => HirType::Bool,
        TypeExpr::Str => HirType::Str,
        TypeExpr::Unit => HirType::Unit,
        TypeExpr::Named(sym) => HirType::Named(*sym),
        TypeExpr::Generic(sym, args) => {
            HirType::Generic(*sym, args.iter().map(|a| lower_type_expr(a, ctx)).collect())
        }
        TypeExpr::Tuple(elems) => {
            HirType::Tuple(elems.iter().map(|e| lower_type_expr(e, ctx)).collect())
        }
        TypeExpr::RawPtr { mutable: _, inner } => {
            HirType::RawPtr(Box::new(lower_type_expr(inner, ctx)))
        }
    }
}


