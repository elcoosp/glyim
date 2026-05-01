use glyim_hir::types::HirType;
use glyim_interner::Interner;

/// Resolves named types to their primitive equivalents
pub fn resolve_named_type(interner: &Interner, ty: &HirType) -> HirType {
    match ty {
        HirType::Named(sym) => match interner.resolve(*sym) {
            "f64" | "Float" => HirType::Float,
            "i64" | "Int" => HirType::Int,
            "bool" | "Bool" => HirType::Bool,
            "Str" | "str" => HirType::Str,
            _ => ty.clone(),
        },
        _ => ty.clone(),
    }
}

/// Checks if a cast between two types is valid
pub fn is_valid_cast(from: &HirType, to: &HirType) -> bool {
    let resolve_fallback = |ty: &HirType| -> HirType {
        match ty {
            HirType::Named(sym) => {
                let name = format!("{:?}", sym);
                if name.contains("f64") || name.contains("Float") {
                    HirType::Float
                } else if name.contains("i64") || name.contains("Int") {
                    HirType::Int
                } else if name.contains("bool") || name.contains("Bool") {
                    HirType::Bool
                } else if name.contains("Str") || name.contains("str") {
                    HirType::Str
                } else {
                    ty.clone()
                }
            }
            _ => ty.clone(),
        }
    };

    let from = resolve_fallback(from);
    let to = resolve_fallback(to);

    match (&from, &to) {
        (HirType::Int, HirType::Float) | (HirType::Float, HirType::Int) => true,
        (HirType::Int, HirType::Int) | (HirType::Float, HirType::Float) => true,
        (_, HirType::RawPtr { .. }) => true,
        (HirType::RawPtr { .. }, _) => true,  // RawPtr -> anything (identity)
        (a, b) if a == b => true,
        _ => false,
    }
}
