//! Diagnostic types and error reporting built on `miette`.

pub use glyim_span::{MultiSpan, Span};
pub use miette::{Diagnostic as MietteDiagnostic, Report, Severity, SourceSpan};

use std::fmt;
use std::sync::Arc;

/// Declared shape of an enum variant, used to carry structured
/// non-exhaustive-match data from `glyim-typeck` to the LSP code action
/// (plan §5.2 / §5.1) so the quick-fix can synthesize an arity-correct arm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VariantShape {
    /// `Variant` — no associated data.
    Unit,
    /// `Variant(a, b)` — `n` positional fields.
    Tuple(usize),
    /// `Variant { x, y }` — named fields.
    Struct(Vec<String>),
}

/// Typed, machine-readable side-data attached to a [`GlyimDiagnostic`],
/// as an alternative to re-parsing the prose `message` (plan §5.2).
#[derive(Clone, Debug)]
pub enum StructuredDiagnosticData {
    /// A `non-exhaustive match` diagnostic carrying each missing variant's
    /// name and shape so the LSP can synthesize compiling match arms.
    MissingMatchVariants(Vec<(String, VariantShape)>),
}

type EmitCallback = Box<dyn FnMut(&GlyimDiagnostic)>;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// ErrorCode.
pub struct ErrorCode {
    /// Struct.
    pub category: ErrorCategory,
    /// Struct.
    pub number: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// ErrorCategory.
pub enum ErrorCategory {
    /// Variant.
    Lex,
    /// Variant.
    Parse,
    /// Variant.
    NameResolution,
    /// Variant.
    Type,
    /// Variant.
    Lifetime,
    /// Variant.
    Borrow,
    /// Variant.
    Comptime,
    /// Variant.
    Io,
    /// Variant.
    Internal,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cat = match self.category {
            ErrorCategory::Lex => "E",
            ErrorCategory::Parse => "P",
            ErrorCategory::NameResolution => "N",
            ErrorCategory::Type => "T",
            ErrorCategory::Lifetime => "L",
            ErrorCategory::Borrow => "B",
            ErrorCategory::Comptime => "C",
            ErrorCategory::Io => "I",
            ErrorCategory::Internal => "X",
        };
        write!(f, "{}{:04}", cat, self.number)
    }
}

#[derive(Clone, Debug)]
/// GlyimDiagnostic.
pub struct GlyimDiagnostic {
    /// Struct.
    pub code: ErrorCode,
    /// Struct.
    pub severity: DiagSeverity,
    /// Struct.
    pub message: String,
    /// Struct.
    pub span: MultiSpan,
    /// Struct.
    pub sub_diagnostics: Vec<SubDiagnostic>,
    /// Struct.
    pub suggestions: Vec<Suggestion>,
    /// Struct.
    pub source_code: Option<Arc<str>>,
    /// Typed, machine-readable side-data (plan §5.2). Optional so diagnostics
    /// constructed without it remain valid; consumers fall back to parsing the
    /// prose `message` when this is `None`.
    pub structured: Option<StructuredDiagnosticData>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// DiagSeverity.
pub enum DiagSeverity {
    /// Variant.
    Error,
    /// Variant.
    Warning,
    /// Variant.
    Note,
    /// Variant.
    Help,
}

impl From<DiagSeverity> for miette::Severity {
    fn from(s: DiagSeverity) -> Self {
        match s {
            DiagSeverity::Error => miette::Severity::Error,
            DiagSeverity::Warning => miette::Severity::Warning,
            DiagSeverity::Note | DiagSeverity::Help => miette::Severity::Advice,
        }
    }
}

#[derive(Clone, Debug)]
/// SubDiagnostic.
pub struct SubDiagnostic {
    /// Struct.
    pub severity: DiagSeverity,
    /// Struct.
    pub message: String,
    /// Struct.
    pub span: Option<MultiSpan>,
}

#[derive(Clone, Debug)]
/// Suggestion.
pub struct Suggestion {
    /// Struct.
    pub message: String,
    #[doc = "field"]
    pub replacements: Vec<(Span, String)>,
    /// Struct.
    pub applicability: Applicability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Applicability.
pub enum Applicability {
    /// Variant.
    MachineApplicable,
    /// Variant.
    MaybeIncorrect,
    /// Variant.
    HasPlaceholders,
    /// Variant.
    Unspecified,
}

impl fmt::Display for GlyimDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for GlyimDiagnostic {}

impl MietteDiagnostic for GlyimDiagnostic {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(self.code))
    }
    fn severity(&self) -> Option<miette::Severity> {
        Some(self.severity.into())
    }
    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        let primary = miette::LabeledSpan::new_with_span(
            Some(self.message.clone()),
            SourceSpan::from(self.span.primary),
        );
        let secondary: Vec<miette::LabeledSpan> = self
            .span
            .secondary
            .iter()
            .map(|(span, label)| {
                miette::LabeledSpan::new_with_span(Some(label.clone()), SourceSpan::from(*span))
            })
            .collect();
        let all: Vec<_> = std::iter::once(primary).chain(secondary).collect();
        Some(Box::new(all.into_iter()))
    }
    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        self.source_code
            .as_ref()
            .map(|arc| arc as &dyn miette::SourceCode)
    }
    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.suggestions
            .first()
            .map(|s| Box::new(s.message.clone()) as Box<dyn fmt::Display>)
    }
}

impl GlyimDiagnostic {
    /// new.
    pub fn new(
        code: ErrorCode,
        severity: DiagSeverity,
        message: impl Into<String>,
        span: MultiSpan,
    ) -> Self {
        Self {
            code,
            severity,
            message: message.into(),
            span,
            sub_diagnostics: Vec::new(),
            suggestions: Vec::new(),
            source_code: None,
            structured: None,
        }
    }

    /// with_source_code.
    pub fn with_source_code(mut self, source: Arc<str>) -> Self {
        self.source_code = Some(source);
        self
    }

    /// lex_error.
    pub fn lex_error(span: Span, message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode {
                category: ErrorCategory::Lex,
                number: 1,
            },
            DiagSeverity::Error,
            message,
            MultiSpan::from_span(span),
        )
    }
    /// parse_error.
    pub fn parse_error(span: Span, message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode {
                category: ErrorCategory::Parse,
                number: 1,
            },
            DiagSeverity::Error,
            message,
            MultiSpan::from_span(span),
        )
    }
    /// type_error.
    pub fn type_error(span: Span, message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode {
                category: ErrorCategory::Type,
                number: 1,
            },
            DiagSeverity::Error,
            message,
            MultiSpan::from_span(span),
        )
    }

    /// Emit when a `match` over an enum fails to cover every variant.
    /// `missing` lists the uncovered variant names; the LSP surfaces a
    /// "Add missing match arm(s)" code action (plan §22.1).
    /// Emit when a `match` is missing arms for some enum variants. The
    /// `shapes` carry each missing variant's name + declared style so the LSP
    /// code action can synthesize an arity-correct arm without re-parsing the
    /// prose `message` (plan §5.2). `missing` is retained for the human-facing
    /// message and for diagnostics consumers that don't read structured data.
    pub fn non_exhaustive_match(
        span: Span,
        missing: &[String],
        shapes: &[(String, VariantShape)],
    ) -> Self {
        let list = missing
            .iter()
            .map(|v| format!("`{}`", v))
            .collect::<Vec<_>>()
            .join(", ");
        let mut diag = Self::new(
            ErrorCode {
                category: ErrorCategory::Type,
                number: 50,
            },
            DiagSeverity::Error,
            format!("non-exhaustive match: missing variants {}", list),
            MultiSpan::from_span(span),
        );
        diag.structured = Some(StructuredDiagnosticData::MissingMatchVariants(
            shapes.to_vec(),
        ));
        diag
    }

    /// Emit when a method/operation requires a trait the receiver type does
    /// not implement. `trait_name`/`type_name` drive the "Generate impl" code
    /// action (plan §22.1).
    pub fn trait_not_implemented(
        span: Span,
        trait_name: impl Into<String>,
        type_name: impl Into<String>,
    ) -> Self {
        Self::new(
            ErrorCode {
                category: ErrorCategory::Type,
                number: 51,
            },
            DiagSeverity::Error,
            format!(
                "trait `{}` is not implemented for `{}`",
                trait_name.into(),
                type_name.into()
            ),
            MultiSpan::from_span(span),
        )
    }
    /// borrow_error.
    pub fn borrow_error(span: Span, message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode {
                category: ErrorCategory::Borrow,
                number: 1,
            },
            DiagSeverity::Error,
            message,
            MultiSpan::from_span(span),
        )
    }
    /// internal_error.
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode {
                category: ErrorCategory::Internal,
                number: 0,
            },
            DiagSeverity::Error,
            message,
            MultiSpan::from_span(Span::DUMMY),
        )
    }
    /// macro_error.
    pub fn macro_error(span: Span, message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode {
                category: ErrorCategory::Comptime,
                number: 1,
            },
            DiagSeverity::Error,
            message,
            MultiSpan::from_span(span),
        )
    }

    /// with_sub.
    pub fn with_sub(mut self, sub: SubDiagnostic) -> Self {
        self.sub_diagnostics.push(sub);
        self
    }
    /// with_suggestion.
    pub fn with_suggestion(mut self, sug: Suggestion) -> Self {
        self.suggestions.push(sug);
        self
    }
    /// is_error.
    pub fn is_error(&self) -> bool {
        matches!(self.severity, DiagSeverity::Error)
    }
}

/// CompResult.
pub type CompResult<T> = Result<T, Vec<GlyimDiagnostic>>;

#[allow(clippy::type_complexity)]
/// DiagSink.
pub struct DiagSink {
    diagnostics: Vec<GlyimDiagnostic>,
    error_count: usize,
    suppressed_count: usize,
    error_limit: usize,
    on_emit: Option<EmitCallback>,
}

impl DiagSink {
    /// new.
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
            error_count: 0,
            suppressed_count: 0,
            error_limit: std::env::var("GLYIM_ERR_LIMIT")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(50),
            // No default `on_emit`: diagnostics are *returned* to the caller
            // (via `into_diagnostics`), and the caller renders them. The
            // previous default logged every diagnostic through `tracing` at
            // `WARN`/`INFO`, so a compile printed each warning twice — once as
            // a `tracing` line and once (if it survived to the CLI) as a real
            // diagnostic — and printed diagnostics for *intermediate* phases
            // that the caller might legitimately suppress. Callers that want
            // live emission can install one via `with_on_emit`.
            on_emit: None,
        }
    }

    /// with_error_limit.
    pub fn with_error_limit(limit: usize) -> Self {
        Self {
            error_limit: limit,
            ..Self::new()
        }
    }

    /// with_on_emit.
    pub fn with_on_emit(on_emit: Option<EmitCallback>) -> Self {
        Self {
            on_emit,
            ..Self::new()
        }
    }

    /// emit.
    pub fn emit(&mut self, diag: GlyimDiagnostic) {
        if diag.is_error() {
            if self.error_count >= self.error_limit {
                self.suppressed_count += 1;
                return;
            }
            self.error_count += 1;
        }
        if let Some(cb) = &mut self.on_emit {
            cb(&diag);
        }
        self.diagnostics.push(diag);
    }

    /// has_errors.
    pub fn has_errors(&self) -> bool {
        self.error_count > 0
    }
    /// diagnostics.
    pub fn diagnostics(&self) -> &[GlyimDiagnostic] {
        &self.diagnostics
    }

    /// into_diagnostics.
    pub fn into_diagnostics(mut self) -> Vec<GlyimDiagnostic> {
        if self.suppressed_count > 0 {
            self.diagnostics
                .push(GlyimDiagnostic::internal_error(format!(
                    "Too many errors emitted; stopping now. ({} errors suppressed)",
                    self.suppressed_count
                )));
        }
        self.diagnostics
    }
}

impl Default for DiagSink {
    fn default() -> Self {
        Self::new()
    }
}

impl Extend<GlyimDiagnostic> for DiagSink {
    fn extend<T: IntoIterator<Item = GlyimDiagnostic>>(&mut self, iter: T) {
        for d in iter {
            self.emit(d);
        }
    }
}

#[cfg(test)]
mod tests;
