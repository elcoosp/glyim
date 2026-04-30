use glyim_syntax::SyntaxKind;
use miette::Diagnostic;

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    #[error("expected {expected} but found {found}")]
    Expected {
        expected: SyntaxKind,
        found: SyntaxKind,
        span: (usize, usize),
    },
    #[error("expected {expected} but reached end of input")]
    UnexpectedEof { expected: SyntaxKind },
    #[error("expected expression but found {found}")]
    ExpectedExpr {
        found: SyntaxKind,
        span: (usize, usize),
    },
    #[error("{msg}")]
    Message { msg: String, span: (usize, usize) },
}

impl Diagnostic for ParseError {
    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        None
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan>>> {
        let (start, end, msg) = match self {
            ParseError::Expected { span, found, .. } => {
                (span.0, span.1, format!("unexpected {}", found))
            }
            ParseError::ExpectedExpr { span, found, .. } => {
                (span.0, span.1, format!("unexpected {}", found))
            }
            ParseError::Message { span, msg } => (span.0, span.1, msg.clone()),
            ParseError::UnexpectedEof { .. } => return None,
        };
        Some(Box::new(std::iter::once(miette::LabeledSpan::new(
            Some(msg),
            start,
            end - start,
        ))))
    }
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
