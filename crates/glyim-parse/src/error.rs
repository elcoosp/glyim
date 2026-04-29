//! Parse error types.
use glyim_syntax::SyntaxKind;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Expected {
        expected: SyntaxKind,
        found: SyntaxKind,
        span: (usize, usize),
    },
    UnexpectedEof {
        expected: SyntaxKind,
    },
    ExpectedExpr {
        found: SyntaxKind,
        span: (usize, usize),
    },
    Message {
        msg: String,
        span: (usize, usize),
    },
}

impl ParseError {
    pub fn expected(
        expected: SyntaxKind,
        found_kind: SyntaxKind,
        start: usize,
        end: usize,
    ) -> Self {
        Self::Expected {
            expected,
            found: found_kind,
            span: (start, end),
        }
    }
    pub fn unexpected_eof(expected: SyntaxKind) -> Self {
        Self::UnexpectedEof { expected }
    }
    pub fn expected_expr(found_kind: SyntaxKind, start: usize, end: usize) -> Self {
        Self::ExpectedExpr {
            found: found_kind,
            span: (start, end),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expected {
                expected,
                found,
                span,
            } => write!(
                f,
                "expected {} but found {} at bytes {}..{}",
                expected.display_name(),
                found.display_name(),
                span.0,
                span.1
            ),
            Self::UnexpectedEof { expected } => write!(
                f,
                "expected {} but reached end of input",
                expected.display_name()
            ),
            Self::ExpectedExpr { found, span } => write!(
                f,
                "expected expression but found {} at bytes {}..{}",
                found.display_name(),
                span.0,
                span.1
            ),
            Self::Message { msg, span } => write!(f, "{} at bytes {}..{}", msg, span.0, span.1),
        }
    }
}
impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_syntax::SyntaxKind;
    #[test]
    fn display_expected() {
        let e = ParseError::expected(SyntaxKind::RParen, SyntaxKind::IntLit, 5, 7);
        let s = e.to_string();
        assert!(s.contains("expected )"));
        assert!(s.contains("but found integer literal"));
        assert!(s.contains("5..7"));
    }
    #[test]
    fn display_eof() {
        let e = ParseError::unexpected_eof(SyntaxKind::RBrace);
        assert!(e.to_string().contains("end of input"));
    }
    #[test]
    fn display_expected_expr() {
        let e = ParseError::expected_expr(SyntaxKind::Semicolon, 10, 11);
        assert!(e.to_string().contains("expected expression"));
    }
    #[test]
    fn display_message() {
        let e = ParseError::Message {
            msg: "oops".into(),
            span: (0, 5),
        };
        assert!(e.to_string().contains("oops"));
    }
    #[test]
    fn is_send_sync() {
        fn assert_ts<T: Send + Sync>() {}
        assert_ts::<ParseError>();
    }
}
