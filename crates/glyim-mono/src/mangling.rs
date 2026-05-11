use glyim_hir::types::HirType;
use glyim_interner::{Interner, Symbol};

const MANGLE_SEPARATOR: &str = "__";
const METHOD_SEPARATOR: &str = "_";

#[derive(Debug, Clone)]
pub enum ManglingError {
    InferInType { type_var_index: u32 },
    ParamInType { symbol_index: u32 },
}

impl std::fmt::Display for ManglingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManglingError::InferInType { type_var_index } => write!(f, "Infer(?{}) reached mangling", type_var_index),
            ManglingError::ParamInType { symbol_index } => write!(f, "Param(symbol {}) reached mangling", symbol_index),
        }
    }
}

impl std::error::Error for ManglingError {}

pub fn type_to_short_string(ty: &HirType, interner: &Interner) -> Result<String, ManglingError> {
    match ty {
        HirType::Named(sym) | HirType::Opaque(sym) => Ok(interner.resolve(*sym).to_string()),
        HirType::Int => Ok("i64".into()),
        HirType::Bool => Ok("bool".into()),
        HirType::Float => Ok("f64".into()),
        HirType::Str => Ok("str".into()),
        HirType::Unit => Ok("unit".into()),
        HirType::Never => Ok("never".into()),
        HirType::Error => Ok("error".into()),
        HirType::Infer(var) => Err(ManglingError::InferInType { type_var_index: var.raw_index() }),
        HirType::Param(sym) => Err(ManglingError::ParamInType { symbol_index: sym.raw() }),
        HirType::Generic(sym, args) => {
            let base = interner.resolve(*sym);
            let mut s = format!("{}", base);
            for arg in args {
                s.push_str(MANGLE_SEPARATOR);
                s.push_str(&type_to_short_string(arg, interner)?);
            }
            Ok(s)
        }
        HirType::Tuple(elems) => {
            let mut s = String::from("tuple");
            for e in elems {
                s.push_str(MANGLE_SEPARATOR);
                s.push_str(&type_to_short_string(e, interner)?);
            }
            Ok(s)
        }
        HirType::RawPtr(inner) => Ok(format!("ptr{}", type_to_short_string(inner, interner)?)),
        HirType::Func(params, ret) => {
            let mut s = format!("fn{}", params.len());
            for p in params {
                s.push_str(MANGLE_SEPARATOR);
                s.push_str(&type_to_short_string(p, interner)?);
            }
            s.push_str(MANGLE_SEPARATOR);
            s.push_str(&type_to_short_string(ret, interner)?);
            Ok(s)
        }
        HirType::Option(inner) => Ok(format!("Option{}", type_to_short_string(inner, interner)?)),
        HirType::Result(ok, err) => Ok(format!("Result{}{}", type_to_short_string(ok, interner)?, type_to_short_string(err, interner)?)),
    }
}

pub fn mangle_name(interner: &mut Interner, base: Symbol, type_args: &[HirType]) -> Result<Symbol, ManglingError> {
    if type_args.is_empty() { return Ok(base); }
    let base_str = interner.resolve(base);
    let mut result = base_str.to_string();
    for arg in type_args {
        result.push_str(MANGLE_SEPARATOR);
        result.push_str(&type_to_short_string(arg, interner)?);
    }
    Ok(interner.intern(&result))
}

pub fn mangle_method_name(interner: &mut Interner, type_name: Symbol, method_name: Symbol, type_args: &[HirType]) -> Result<Symbol, ManglingError> {
    let type_str = interner.resolve(type_name);
    let method_str = interner.resolve(method_name);
    let base = format!("{}{}{}", type_str, METHOD_SEPARATOR, method_str);
    let base_sym = interner.intern(&base);
    mangle_name(interner, base_sym, type_args)
}
