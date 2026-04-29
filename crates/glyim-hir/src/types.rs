//! Type system types and patterns for the HIR.

use glyim_interner::Symbol;

/// Unique identifier for an expression node, used for type annotation lookups.
pub type ExprId = u32;

/// High-level types in the Glyim type system.
#[derive(Debug, Clone, PartialEq)]
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
    },
    /// Enum variant pattern `Shape::Circle(r)`
    EnumVariant {
        enum_name: Symbol,
        variant_name: Symbol,
        bindings: Vec<(Symbol, HirPattern)>,
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
