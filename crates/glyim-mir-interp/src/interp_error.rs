#[derive(Debug, PartialEq)]
/// InterpError.
pub enum InterpError {
    /// Variant.
    TimedOut,
    /// Variant.
    StackOverflow,
    /// Integer (or float) division / remainder by zero (de-stubbing plan §11.2).
    DivisionByZero,
    #[allow(missing_docs)]
    Panic(String),
    /// Cross-frame unwind reached the top of the call stack
    /// (de-stubbing plan §7.2). Carries the original panic payload so callers
    /// (and the interpreter test harness) can inspect *which* panic propagated
    /// all the way up after every intermediate frame's cleanup block ran.
    Unwind(Box<InterpError>),
}

impl std::fmt::Display for InterpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut => write!(f, "interpreter timed out"),
            Self::StackOverflow => write!(f, "stack overflow"),
            Self::DivisionByZero => write!(
                f,
                "attempt to calculate remainder/division with a divisor of zero"
            ),
            Self::Panic(msg) => write!(f, "panic: {}", msg),
            Self::Unwind(inner) => write!(f, "unwind: {}", inner),
        }
    }
}

impl std::error::Error for InterpError {}
