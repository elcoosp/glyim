use crate::*;

#[test]
fn test_diag_assertions() {
    let diags = vec![
        glyim_diag::GlyimDiagnostic::type_error(glyim_span::Span::DUMMY, "test error"),
        glyim_diag::GlyimDiagnostic::new(
            glyim_diag::ErrorCode::from_registered(glyim_diag::ErrorCategory::Type, 2),
            glyim_diag::DiagSeverity::Warning,
            "test warning",
            glyim_diag::MultiSpan::from_span(glyim_span::Span::DUMMY),
        ),
    ];
    assert_has_errors(&diags);
    assert_error_count(&diags, 1);
    assert_diag_contains(&diags, "test error");
    assert_has_severity(&diags, glyim_diag::DiagSeverity::Warning);
}

#[test]
fn test_assert_no_errors() {
    let diags = vec![glyim_diag::GlyimDiagnostic::new(
        glyim_diag::ErrorCode::from_registered(glyim_diag::ErrorCategory::Type, 1),
        glyim_diag::DiagSeverity::Warning,
        "just a warning",
        glyim_diag::MultiSpan::from_span(glyim_span::Span::DUMMY),
    )];
    assert_no_errors(&diags);
}
