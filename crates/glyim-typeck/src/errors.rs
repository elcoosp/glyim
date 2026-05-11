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
    UnresolvedName { name: Symbol, span: Span },
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
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{:?}", self) }
}
impl std::error::Error for TypeError {}

#[derive(Debug, Clone)]
pub enum UnifyError {
    Mismatch { expected: HirType, found: HirType, expected_span: Span, found_span: Span },
    InfiniteType { span: Span },
    ResolveDepthExceeded { type_var: TypeVar, span: Span },
}

impl std::fmt::Display for UnifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{:?}", self) }
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
