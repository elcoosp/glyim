use glyim_hir::types::HirType;
use glyim_interner::Interner;

pub fn format_type_for_error(ty: &HirType, interner: &Interner) -> String {
    match ty {
        HirType::Unit => "()".into(),
        HirType::Never => "!".into(),
        HirType::Error => "<error>".into(),
        HirType::Infer(var) => format!("?{}", var.raw_index()),
        HirType::Param(sym) | HirType::Named(sym) | HirType::Opaque(sym) => interner.resolve(*sym).to_string(),
        HirType::Int => "Int".into(),
        HirType::Bool => "Bool".into(),
        HirType::Float => "Float".into(),
        HirType::Str => "Str".into(),
        HirType::Generic(sym, args) => {
            let a = args.iter().map(|a| format_type_for_error(a, interner)).collect::<Vec<_>>().join(", ");
            format!("{}<{}>", interner.resolve(*sym), a)
        }
        HirType::RawPtr(inner) => format!("*{}", format_type_for_error(inner, interner)),
        HirType::Tuple(elems) => format!("({})", elems.iter().map(|a| format_type_for_error(a, interner)).collect::<Vec<_>>().join(", ")),
        HirType::Func(params, ret) => format!("fn({}) -> {}", params.iter().map(|a| format_type_for_error(a, interner)).collect::<Vec<_>>().join(", "), format_type_for_error(ret, interner)),
        HirType::Option(inner) => format!("Option<{}>", format_type_for_error(inner, interner)),
        HirType::Result(ok, err) => format!("Result<{}, {}>", format_type_for_error(ok, interner), format_type_for_error(err, interner)),
    }
}
