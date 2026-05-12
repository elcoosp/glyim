use crate::lower::context::LoweringContext;
use crate::types::HirType;
use glyim_parse::TypeExpr;

/// Map well-known type names to their canonical Glyim equivalents.
/// This ensures that u8, i32, *const u8, etc. all resolve to the
/// same underlying types as i64/Int.
fn canonicalize_type_name(name: &str) -> Option<HirType> {
    match name {
        // Integer types → i64
        "i8" | "i16" | "i32" | "i64" | "Int" | "u8" | "u16" | "u32" | "u64" | "usize" => {
            Some(HirType::Int)
        }
        // Float types → f64
        "f32" | "f64" | "Float" => Some(HirType::Float),
        // Boolean
        "bool" | "Bool" => Some(HirType::Bool),
        // String (but not the fat pointer Str type)
        // "str" | "Str" => Some(HirType::Str),
        _ => None,
    }
}

pub(crate) fn lower_type_expr(ty: &TypeExpr, ctx: &mut LoweringContext) -> HirType {
    match ty {
        TypeExpr::Int => HirType::Int,
        TypeExpr::Float => HirType::Float,
        TypeExpr::Bool => HirType::Bool,
        TypeExpr::Str => HirType::Str,
        TypeExpr::Unit => HirType::Unit,
        TypeExpr::Named(sym) => {
            if ctx.is_type_param(*sym) {
                HirType::Param(*sym)
            } else {
                let name = ctx.resolve(*sym);
                // Canonicalize the type name if it's a known alias
                canonicalize_type_name(name).unwrap_or(HirType::Named(*sym))
            }
        }
        TypeExpr::Generic(sym, args) => {
            let name = ctx.resolve(*sym);
            // Check if the base is a canonicalizable type
            if let Some(canonical) = canonicalize_type_name(name) {
                match canonical {
                    HirType::Int | HirType::Float | HirType::Bool | HirType::Str => {
                        // For primitives, Generic doesn't make sense, just return the primitive
                        canonical
                    }
                    _ => HirType::Generic(
                        *sym,
                        args.iter().map(|a| lower_type_expr(a, ctx)).collect(),
                    ),
                }
            } else {
                HirType::Generic(*sym, args.iter().map(|a| lower_type_expr(a, ctx)).collect())
            }
        }
        TypeExpr::Tuple(elems) => {
            HirType::Tuple(elems.iter().map(|e| lower_type_expr(e, ctx)).collect())
        }
        TypeExpr::RawPtr { mutable: _, inner } => {
            // For pointer types, canonicalize the inner type too
            let inner_ty = lower_type_expr(inner, ctx);
            HirType::RawPtr(Box::new(inner_ty))
        }
    }
}
