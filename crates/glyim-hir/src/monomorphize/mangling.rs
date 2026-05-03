use crate::types::HirType;
use glyim_interner::{Interner, Symbol};

pub fn type_to_short_string(ty: &HirType, interner: &Interner) -> String {
    match ty {
        HirType::Int => "i64".to_string(),
        HirType::Bool => "bool".to_string(),
        HirType::Float => "f64".to_string(),
        HirType::Str => "str".to_string(),
        HirType::Unit => "unit".to_string(),
        HirType::Never => "never".to_string(),
        HirType::Named(s) => interner.resolve(*s).to_string(),
        HirType::Generic(s, args) => {
            let inner = args
                .iter()
                .map(|a| type_to_short_string(a, interner))
                .collect::<Vec<_>>()
                .join("_");
            format!("{}_{}", interner.resolve(*s), inner)
        }
        HirType::Tuple(elems) => format!(
            "tup_{}",
            elems
                .iter()
                .map(|e| type_to_short_string(e, interner))
                .collect::<Vec<_>>()
                .join("_")
        ),
        HirType::RawPtr(inner) => format!("ptr_{}", type_to_short_string(inner, interner)),
        HirType::Option(inner) => format!("opt_{}", type_to_short_string(inner, interner)),
        HirType::Result(ok, err) => format!(
            "res_{}_{}",
            type_to_short_string(ok, interner),
            type_to_short_string(err, interner)
        ),
        _ => format!("ty{:?}", std::mem::discriminant(ty)),
    }
}

pub fn mangle_type_name(interner: &mut Interner, base: Symbol, type_args: &[HirType]) -> Symbol {
    let base_str = interner.resolve(base).to_string();
    let args_str = type_args
        .iter()
        .map(|t| type_to_short_string(t, interner))
        .collect::<Vec<_>>()
        .join("_");
    interner.intern(&format!("{}__{}", base_str, args_str))
}
