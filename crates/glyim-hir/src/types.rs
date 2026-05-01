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

/// High-level types in the Glyim type system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirType {
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

/// Substitute type parameters with concrete types.
/// `sub` maps type parameter symbols to their concrete types.
pub fn substitute_type(ty: &HirType, sub: &HashMap<Symbol, HirType>) -> HirType {
    match ty {
        HirType::Named(sym) => sub.get(sym).cloned().unwrap_or_else(|| ty.clone()),
        HirType::Generic(sym, args) => {
            let new_args: Vec<HirType> = args.iter().map(|a| substitute_type(a, sub)).collect();
            // If all args are now concrete (no type params remain), just return Named
            let has_params = new_args
                .iter()
                .any(|a| matches!(a, HirType::Named(s) if sub.contains_key(s)));
            if !has_params && !new_args.is_empty() {
                HirType::Generic(*sym, new_args)
            } else {
                HirType::Generic(*sym, new_args)
            }
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
        HirType::Func(params, ret) => HirType::Func(
            params.iter().map(|p| substitute_type(p, sub)).collect(),
            Box::new(substitute_type(ret, sub)),
        ),
        _ => ty.clone(),
    }
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
