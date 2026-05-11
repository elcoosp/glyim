use glyim_hir::types::{HirType, TypeVar};
use glyim_diag::Span;
use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq)]
pub enum InferKind { Variable, Return, Closure, GenericArg, LetBinding, ForInIterator }

impl std::fmt::Display for InferKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InferKind::Variable => write!(f, "variable"),
            InferKind::Return => write!(f, "return type"),
            InferKind::Closure => write!(f, "closure"),
            InferKind::GenericArg => write!(f, "generic argument"),
            InferKind::LetBinding => write!(f, "let binding"),
            InferKind::ForInIterator => write!(f, "for-in iterator"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeError {
    MismatchedTypes {
        expected: Box<HirType>,
        found: Box<HirType>,
        expected_span: Span,
        found_span: Span,
    },
    UnresolvedName { name: String, span: Span },
    UnresolvedMethod {
        method_name: Symbol,
        receiver_type: Box<HirType>,
        span: Span,
    },
    CannotInferType {
        kind: InferKind,
        type_var: TypeVar,
        span: Span,
    },
    InfiniteType { span: Span },
    ResolveDepthExceeded { type_var: TypeVar, span: Span },
    ArgumentCountMismatch {
        expected: usize,
        actual: usize,
        span: Span,
    },
    ShapeMismatch {
        expected: Box<HirType>,
        found: Box<HirType>,
        expected_span: Span,
        found_span: Span,
    },
    UnresolvedFieldOnInfer {
        type_var: TypeVar,
        field: Symbol,
        span: Span,
    },
    // Legacy variant names for backward compat with snapshot tests
    UnknownField {
        struct_name: String,
        field: String,
        span: (usize, usize),
    },
    MissingField {
        struct_name: String,
        field: String,
        span: (usize, usize),
    },
    NonExhaustiveMatch {
        missing: Vec<String>,
        span: (usize, usize),
    },
    AssignToImmutable {
        name: String,
        expr_id: glyim_hir::types::ExprId,
        span: (usize, usize),
    },
    AssignThroughNonPointer {
        found: HirType,
        expr_id: glyim_hir::types::ExprId,
        span: (usize, usize),
    },
    DerefNonPointer {
        found: HirType,
        expr_id: glyim_hir::types::ExprId,
        span: (usize, usize),
    },
    InvalidReturnType {
        expected: HirType,
        found: HirType,
    },
    InvalidQuestion {
        expr_id: glyim_hir::types::ExprId,
    },
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeError::MismatchedTypes { expected, found, .. } => {
                write!(f, "type mismatch: expected {:?}, found {:?}", expected, found)
            }
            TypeError::UnresolvedName { name, .. } => {
                write!(f, "unresolved name `{}`", name)
            }
            TypeError::UnresolvedMethod { method_name, receiver_type, .. } => {
                write!(f, "unresolved method `{:?}` on type `{:?}`", method_name, receiver_type)
            }
            TypeError::CannotInferType { kind, type_var, .. } => {
                write!(f, "cannot infer type for {} (?{})", kind, type_var.raw_index())
            }
            TypeError::InfiniteType { .. } => {
                write!(f, "infinite type detected")
            }
            TypeError::ResolveDepthExceeded { type_var, .. } => {
                write!(f, "resolve depth exceeded for ?{}", type_var.raw_index())
            }
            TypeError::ArgumentCountMismatch { expected, actual, .. } => {
                write!(f, "argument count mismatch: expected {}, got {}", expected, actual)
            }
            TypeError::ShapeMismatch { expected, found, .. } => {
                write!(f, "shape mismatch: expected {:?}, found {:?}", expected, found)
            }
            TypeError::UnresolvedFieldOnInfer { field, .. } => {
                write!(f, "cannot access field `{:?}` on inferred type", field)
            }
            TypeError::UnknownField { struct_name, field, .. } => {
                write!(f, "unknown field `{}` on struct `{}`", field, struct_name)
            }
            TypeError::MissingField { struct_name, field, .. } => {
                write!(f, "missing field `{}` in struct `{}`", field, struct_name)
            }
            TypeError::NonExhaustiveMatch { missing, .. } => {
                write!(f, "non-exhaustive match, missing variants: {:?}", missing)
            }
            TypeError::AssignToImmutable { name, .. } => {
                write!(f, "cannot assign to immutable `{}`", name)
            }
            TypeError::AssignThroughNonPointer { found, .. } => {
                write!(f, "cannot assign through non-pointer type `{:?}`", found)
            }
            TypeError::DerefNonPointer { found, .. } => {
                write!(f, "cannot dereference non-pointer type `{:?}`", found)
            }
            TypeError::InvalidReturnType { expected, found } => {
                write!(f, "invalid return type: expected {:?}, found {:?}", expected, found)
            }
            TypeError::InvalidQuestion { .. } => {
                write!(f, "? operator used outside of Result-returning function")
            }
        }
    }
}

impl std::error::Error for TypeError {}

#[derive(Debug, Clone)]
pub enum UnifyError {
    Mismatch { expected: HirType, found: HirType, expected_span: Span, found_span: Span },
    InfiniteType { span: Span },
    ResolveDepthExceeded { type_var: TypeVar, span: Span },
}

impl std::fmt::Display for UnifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl std::error::Error for UnifyError {}

impl UnifyError {
    pub fn into_type_error(self) -> TypeError {
        match self {
            UnifyError::Mismatch { expected, found, expected_span, found_span } =>
                TypeError::MismatchedTypes { expected: Box::new(expected), found: Box::new(found), expected_span, found_span },
            UnifyError::InfiniteType { span } => TypeError::InfiniteType { span },
            UnifyError::ResolveDepthExceeded { type_var, span } => TypeError::ResolveDepthExceeded { type_var, span },
        }
    }
}
