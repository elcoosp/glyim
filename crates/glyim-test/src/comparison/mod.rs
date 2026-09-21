/// normalize.
pub mod normalize;

use crate::annotations::Annotation;
use glyim_diag::{DiagSeverity, GlyimDiagnostic};

/// DiagSeverityExt.
pub trait DiagSeverityExt {
    /// display_name.
    fn display_name(self) -> &'static str;
}

impl DiagSeverityExt for DiagSeverity {
    fn display_name(self) -> &'static str {
        match self {
            DiagSeverity::Error => "ERROR",
            DiagSeverity::Warning => "WARNING",
            DiagSeverity::Note => "NOTE",
            DiagSeverity::Help => "HELP",
        }
    }
}

#[derive(Clone, Debug)]
/// NormalizedDiag.
pub struct NormalizedDiag {
    /// Struct.
    pub severity: DiagSeverity,
    /// Struct.
    pub line: usize,
    /// Struct.
    pub message: String,
}

impl NormalizedDiag {
    /// from_glyim_diag.
    pub fn from_glyim_diag(diag: &GlyimDiagnostic, source: &str) -> Self {
        let line = byte_offset_to_line(source, diag.span.primary.lo.to_usize());
        Self {
            severity: diag.severity,
            line,
            message: diag.message.clone(),
        }
    }
}

#[derive(Clone, Debug)]
/// ComparisonResult.
pub struct ComparisonResult {
    /// Struct.
    pub matched: Vec<MatchedPair>,
    /// Struct.
    pub missing: Vec<Annotation>,
    /// Struct.
    pub unexpected: Vec<NormalizedDiag>,
    /// Struct.
    pub wrong_severity: Vec<SeverityMismatch>,
    /// Struct.
    pub optional_unmatched: Vec<Annotation>,
}

impl ComparisonResult {
    /// passed.
    pub fn passed(&self) -> bool {
        self.missing.is_empty() && self.unexpected.is_empty() && self.wrong_severity.is_empty()
    }
}

#[derive(Clone, Debug)]
/// MatchedPair.
pub struct MatchedPair {
    /// Struct.
    pub annotation: Annotation,
    /// Struct.
    pub diagnostic: NormalizedDiag,
}

#[derive(Clone, Debug)]
/// SeverityMismatch.
pub struct SeverityMismatch {
    /// Struct.
    pub annotation: Annotation,
    /// Struct.
    pub diagnostic: NormalizedDiag,
    /// Struct.
    pub expected: DiagSeverity,
    /// Struct.
    pub actual: DiagSeverity,
}

/// compare_diagnostics.
pub fn compare_diagnostics(
    annotations: &[Annotation],
    diagnostics: &[NormalizedDiag],
) -> ComparisonResult {
    let mut matched = Vec::new();
    let mut missing = Vec::new();
    let mut wrong_severity = Vec::new();
    let mut optional_unmatched = Vec::new();
    let mut diag_used = vec![false; diagnostics.len()];

    for annotation in annotations {
        let target_line = annotation.target_line();
        let mut found = false;

        for (i, diag) in diagnostics.iter().enumerate() {
            if diag_used[i] {
                continue;
            }

            let line_matches = if annotation.fuzzy {
                diag.line.abs_diff(target_line) <= 1
            } else {
                diag.line == target_line
            };

            if line_matches && annotation.pattern.matches(&diag.message) {
                diag_used[i] = true;
                found = true;
                if diag.severity == annotation.severity {
                    matched.push(MatchedPair {
                        annotation: annotation.clone(),
                        diagnostic: diag.clone(),
                    });
                } else {
                    wrong_severity.push(SeverityMismatch {
                        annotation: annotation.clone(),
                        diagnostic: diag.clone(),
                        expected: annotation.severity,
                        actual: diag.severity,
                    });
                }
                break;
            }
        }

        if !found {
            if annotation.optional {
                optional_unmatched.push(annotation.clone());
            } else {
                missing.push(annotation.clone());
            }
        }
    }

    let unexpected: Vec<NormalizedDiag> = diagnostics
        .iter()
        .enumerate()
        .filter(|(i, _)| !diag_used[*i])
        .map(|(_, d)| d.clone())
        .collect();

    ComparisonResult {
        matched,
        missing,
        unexpected,
        wrong_severity,
        optional_unmatched,
    }
}

fn byte_offset_to_line(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .chars()
        .filter(|&c| c == '\n')
        .count()
}
