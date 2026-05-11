//! Type system types and patterns for the HIR.

use glyim_diag::Span;
use glyim_interner::Symbol;
use std::collections::HashMap;

/// Unique identifier for an expression node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(u32);

impl ExprId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn as_usize(self) -> usize {
        self.0 as usize
    }
}

/// Type inference variable (unique per fresh unification variable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct TypeVar(u32);

impl TypeVar {
    #[inline]
    pub fn from_raw_unchecked(index: u32) -> Self { Self(index) }
    #[inline]
    pub fn raw_index(self) -> u32 { self.0 }
}

/// High-level types in the Glyim type system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum HirType {
    Infer(TypeVar),
    Param(Symbol),
    /// 64-bit signed integer
    Int,
    /// Boolean (i1 in LLVM, zero-extended to i64 for uniform representation)
    Bool,
    /// 64-bit IEEE 754 double-precision float
    Float,
    /// String fat pointer { i8*, i64 }
    Str,
    /// Unit / empty tuple `()` — zero-size type
    Unit,
    /// User-defined struct or enum (by name)
    Named(Symbol),
    /// Generic instantiation: `Vec<Int>`, `HashMap<Str, Int>`
    Generic(Symbol, Vec<HirType>),
    /// Tuple type: `(Int, Str, Bool)`
    Tuple(Vec<HirType>),
    /// Raw pointer (*const T or *mut T)
    RawPtr(Box<HirType>),
    /// Opaque @rust("…") type — pointer-sized
    Opaque(Symbol),
    /// Function type (parameters, return type)
    Func(Vec<HirType>, Box<HirType>),
    /// Monomorphized Option<T> (compiler-internal)
    Option(Box<HirType>),
    /// Monomorphized Result<T, E> (compiler-internal)
    Result(Box<HirType>, Box<HirType>),
    /// Uninhabited type (for diverging expressions)
    Never,
    /// Error type for type error recovery. Suppresses cascading errors.
    /// Has the "propagation" property: any operation on this type
    /// produces Error without emitting additional diagnostics.
    Error,
}

/// Patterns used in match arms and destructuring.
#[derive(Debug, Clone, PartialEq)]
pub enum HirPattern {
    /// Wildcard `_`
    Wild,
    /// Boolean literal
    BoolLit(bool),
    /// Integer literal
    IntLit(i64),
    /// Float literal
    FloatLit(f64),
    /// String literal
    StrLit(String),
    /// Unit `()`
    Unit,
    /// Variable binding
    Var(Symbol),
    /// Struct pattern `Point { x, y }`
    Struct {
        name: Symbol,
        bindings: Vec<(Symbol, HirPattern)>,
        span: Span,
    },
    /// Enum variant pattern `Shape::Circle(r)`
    EnumVariant {
        enum_name: Symbol,
        variant_name: Symbol,
        bindings: Vec<(Symbol, HirPattern)>,
        span: Span,
    },
    /// Tuple pattern: `(a, _, b)`
    Tuple {
        elements: Vec<HirPattern>,
        span: Span,
    },
    /// Some(x)
    OptionSome(Box<HirPattern>),
    /// None
    OptionNone,
    /// Ok(x)
    ResultOk(Box<HirPattern>),
    /// Err(e)
    ResultErr(Box<HirPattern>),
}

impl HirType {
    pub fn has_infer(&self) -> bool {
        match self {
            HirType::Infer(_) => true,
            HirType::Param(_) | HirType::Named(_) | HirType::Int | HirType::Bool | HirType::Float | HirType::Str | HirType::Unit | HirType::Never | HirType::Error | HirType::Opaque(_) => false,
            HirType::Generic(_, args) => args.iter().any(|a| a.has_infer()),
            HirType::Tuple(elems) => elems.iter().any(|e| e.has_infer()),
            HirType::RawPtr(inner) => inner.has_infer(),
            HirType::Func(params, ret) => params.iter().any(|p| p.has_infer()) || ret.has_infer(),
            _ => false,
        }
    }
    pub fn has_infer_or_error(&self) -> bool {
        match self {
            HirType::Infer(_) | HirType::Error => true,
            HirType::Param(_) | HirType::Named(_) | HirType::Int | HirType::Bool | HirType::Float | HirType::Str | HirType::Unit | HirType::Never | HirType::Opaque(_) => false,
            HirType::Generic(_, args) => args.iter().any(|a| a.has_infer_or_error()),
            HirType::Tuple(elems) => elems.iter().any(|e| e.has_infer_or_error()),
            HirType::RawPtr(inner) => inner.has_infer_or_error(),
            HirType::Func(params, ret) => params.iter().any(|p| p.has_infer_or_error()) || ret.has_infer_or_error(),
            _ => false,
        }
    }
    pub fn has_param(&self) -> bool {
        match self {
            HirType::Param(_) => true,
            HirType::Infer(_) | HirType::Named(_) | HirType::Int | HirType::Bool | HirType::Float | HirType::Str | HirType::Unit | HirType::Never | HirType::Error | HirType::Opaque(_) => false,
            HirType::Generic(_, args) => args.iter().any(|a| a.has_param()),
            HirType::Tuple(elems) => elems.iter().any(|e| e.has_param()),
            HirType::RawPtr(inner) => inner.has_param(),
            HirType::Func(params, ret) => params.iter().any(|p| p.has_param()) || ret.has_param(),
            _ => false,
        }
    }
}

/// Substitute type parameters with concrete types.
/// `sub` maps type parameter symbols to their concrete types.
pub fn substitute_type(ty: &HirType, sub: &HashMap<Symbol, HirType>) -> HirType {
    match ty {
        HirType::Param(sym) | HirType::Named(sym) => sub.get(sym).cloned().unwrap_or_else(|| ty.clone()),
        HirType::Generic(sym, args) => {
            let new_args: Vec<HirType> = args.iter().map(|a| substitute_type(a, sub)).collect();
            // If all args are now concrete (no type params remain), just return Named
            let _has_params = new_args
                .iter()
                .any(|a| matches!(a, HirType::Named(s) if sub.contains_key(s)));
            HirType::Generic(*sym, new_args)
        }
        HirType::Tuple(elems) => {
            HirType::Tuple(elems.iter().map(|e| substitute_type(e, sub)).collect())
        }
        HirType::RawPtr(inner) => HirType::RawPtr(Box::new(substitute_type(inner, sub))),
        HirType::Option(inner) => HirType::Option(Box::new(substitute_type(inner, sub))),
        HirType::Result(ok, err) => HirType::Result(
            Box::new(substitute_type(ok, sub)),
            Box::new(substitute_type(err, sub)),
        ),
        HirType::Error => HirType::Error,
        HirType::Func(params, ret) => HirType::Func(
            params.iter().map(|p| substitute_type(p, sub)).collect(),
            Box::new(substitute_type(ret, sub)),
        ),
        _ => ty.clone(),
    }
}


const MAX_SUBST_DEPTH: u32 = 256;

#[derive(Debug, Clone, PartialEq)]
pub enum SubstitutionError { DepthExceeded }

pub fn substitute_type_with<F>(ty: &HirType, f: &mut F, depth: u32) -> Result<HirType, SubstitutionError>
where F: FnMut(&Symbol) -> Option<HirType> {
    if depth > MAX_SUBST_DEPTH { return Err(SubstitutionError::DepthExceeded); }
    match ty {
        HirType::Param(sym) => Ok(f(sym).unwrap_or_else(|| ty.clone())),
        HirType::Generic(sym, args) => {
            let new_args = args.iter().map(|a| substitute_type_with(a, f, depth + 1)).collect::<Result<Vec<_>, _>>()?;
            Ok(HirType::Generic(*sym, new_args))
        }
        HirType::Tuple(elems) => {
            let new_elems = elems.iter().map(|e| substitute_type_with(e, f, depth + 1)).collect::<Result<Vec<_>, _>>()?;
            Ok(HirType::Tuple(new_elems))
        }
        HirType::RawPtr(inner) => Ok(HirType::RawPtr(Box::new(substitute_type_with(inner, f, depth + 1)?))),
        HirType::Func(params, ret) => {
            let new_params = params.iter().map(|p| substitute_type_with(p, f, depth + 1)).collect::<Result<Vec<_>, _>>()?;
            let new_ret = substitute_type_with(ret, f, depth + 1)?;
            Ok(HirType::Func(new_params, Box::new(new_ret)))
        }
        _ => Ok(ty.clone()),
    }
}

pub fn substitute_type_safe(ty: &HirType, sub: &HashMap<Symbol, HirType>) -> Result<HirType, SubstitutionError> {
    substitute_type_with(ty, &mut |sym| sub.get(sym).cloned(), 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_interner::Interner;

    #[test]
    fn generic_type_with_single_param() {
        let mut i = Interner::new();
        let sym = i.intern("Vec");
        let t = HirType::Generic(sym, vec![HirType::Int]);
        assert_eq!(t, HirType::Generic(sym, vec![HirType::Int]));
    }

    #[test]
    fn generic_type_with_multiple_params() {
        let mut i = Interner::new();
        let sym = i.intern("HashMap");
        let t = HirType::Generic(sym, vec![HirType::Str, HirType::Int]);
        assert!(matches!(t, HirType::Generic(_, ref params) if params.len() == 2));
    }

    #[test]
    fn tuple_type() {
        let t = HirType::Tuple(vec![HirType::Int, HirType::Str]);
        assert!(matches!(t, HirType::Tuple(ref elems) if elems.len() == 2));
    }

    #[test]
    fn tuple_unit_is_distinct_from_unit() {
        let tuple_unit = HirType::Tuple(vec![]);
        assert_ne!(HirType::Unit, tuple_unit);
    }

    #[test]
    fn expr_id_from_raw() {
        let id = ExprId::new(42);
        assert_eq!(id.as_usize(), 42);
    }

    #[test]
    fn expr_id_is_copy() {
        let id = ExprId::new(5);
        let _a = id;
        let _b = id;
    }

    #[test]
    fn expr_id_is_send_sync() {
        fn assert_ts<T: Send + Sync>() {}
        assert_ts::<ExprId>();
    }

    #[test]
    fn tuple_pattern() {
        let mut i = Interner::new();
        let a = i.intern("a");
        let b = i.intern("b");
        let p = HirPattern::Tuple {
            elements: vec![HirPattern::Var(a), HirPattern::Wild, HirPattern::Var(b)],
            span: Span::new(0, 0),
        };
        assert!(matches!(p, HirPattern::Tuple { ref elements, .. } if elements.len() == 3));
    }
}
