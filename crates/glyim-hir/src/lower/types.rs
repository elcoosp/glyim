use crate::lower::context::LoweringContext;
use crate::types::HirType;
use glyim_parse::TypeExpr;

pub fn lower_type_expr(ty: &TypeExpr, ctx: &mut LoweringContext) -> HirType {
    eprintln!("[lower_type_expr] INPUT ty={:?}", ty);
    let result = match ty {
        TypeExpr::Int => HirType::Int,
        TypeExpr::Float => HirType::Float,
        TypeExpr::Bool => HirType::Bool,
        TypeExpr::Str => HirType::Str,
        TypeExpr::Unit => HirType::Unit,
        TypeExpr::Named(sym) => {
            eprintln!(
                "[lower_type_expr] Named symbol: {:?}, is_type_param={}",
                ctx.interner.resolve(*sym),
                ctx.is_type_param(*sym)
            );
            if ctx.is_type_param(*sym) {
                HirType::Param(*sym)
            } else {
                HirType::Named(*sym)
            }
        }
        TypeExpr::Generic(sym, args) => {
            eprintln!(
                "[lower_type_expr] Generic symbol: {:?}, args={:?}",
                ctx.interner.resolve(*sym),
                args
            );
            HirType::Generic(*sym, args.iter().map(|a| lower_type_expr(a, ctx)).collect())
        }
        TypeExpr::Tuple(elems) => {
            HirType::Tuple(elems.iter().map(|e| lower_type_expr(e, ctx)).collect())
        }
        TypeExpr::RawPtr { mutable: _, inner } => {
            HirType::RawPtr(Box::new(lower_type_expr(inner, ctx)))
        }
    };
    eprintln!("[lower_type_expr] RESULT ty={:?}", result);
    result
}
