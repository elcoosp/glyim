//! GSX diagnostics (design doc §5.2, §7.7).
//!
//! GSX violations are compile errors with fix-its. Codes follow the design
//! doc's examples: `G0201` for untrackable slot reads (Signal required), and
//! the schema-family `G01xx` for prop/event/children/attr validation.

/// Diagnostic code family for GSX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GsxCode {
    /// Schema: unknown prop for the target element/component (§5.2).
    UnknownProp,
    /// Schema: unknown event (§5.2 `#[event]`).
    UnknownEvent,
    /// Schema: content-model violation (§5.2 `content = …`).
    ContentModel,
    /// Schema: required accessibility prop missing (§5.2
    /// `#[a11y(required_if(no_text_child))]`).
    A11yRequired,
    /// Duplicate attribute on one element.
    DuplicateAttr,
    /// Handler is a capturing closure; GSX handlers are state transformers
    /// `fn(&mut S, E)` (§7.3).
    HandlerCapture,
    /// Slot reads something the compiler cannot track via exclusive paths —
    /// wrap in `Signal<T>` (§7.7).
    UntrackableRead,
    /// Structural write into a keyed region's container (§7.6).
    StructuralWrite,
}

impl GsxCode {
    /// The `Gxxxx` code as printed in diagnostics.
    pub fn code(self) -> u32 {
        match self {
            GsxCode::UnknownProp => 0_0101,
            GsxCode::UnknownEvent => 0_0102,
            GsxCode::ContentModel => 0_0103,
            GsxCode::A11yRequired => 0_0104,
            GsxCode::DuplicateAttr => 0_0105,
            GsxCode::HandlerCapture => 0_0200,
            GsxCode::UntrackableRead => 0_0201,
            GsxCode::StructuralWrite => 0_0202,
        }
    }

    /// `Gxxxx` string form, as the design doc prints them (`error[G0201]`).
    pub fn display_code(self) -> String {
        format!("G{:04}", self.code())
    }
}

/// One GSX compile-time diagnostic with an optional fix-it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GsxDiagnostic {
    /// Struct.
    pub code: GsxCode,
    /// Human message (`error[G0201]: …`).
    pub message: String,
    /// Byte range in the source file, when known.
    pub range: Option<(usize, usize)>,
    /// Machine-applicable fix-it text (§7.7: "wrap in `Signal<i32>`").
    pub fixit: Option<String>,
}

impl GsxDiagnostic {
    pub(crate) fn new(code: GsxCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            range: None,
            fixit: None,
        }
    }

    /// Attach a byte range.
    pub fn at(mut self, range: (usize, usize)) -> Self {
        self.range = Some(range);
        self
    }

    /// Attach a fix-it suggestion.
    pub fn with_fixit(mut self, fixit: impl Into<String>) -> Self {
        self.fixit = Some(fixit.into());
        self
    }

    /// `error[G0201]: slot reads …` — the full rendered form.
    pub fn render(&self) -> String {
        format!("error[{}]: {}", self.code.display_code(), self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §7.7 example: `error[G0201]: slot reads `Cell<i32>`, which the compiler
    /// cannot track` — the exact code and a fix-it are part of the contract.
    #[test]
    fn g0201_rendering_matches_design_doc() {
        let d = GsxDiagnostic::new(
            GsxCode::UntrackableRead,
            "slot reads `Cell<i32>`, which the compiler cannot track",
        )
        .with_fixit("wrap in `Signal<i32>`; writes will bump a bit in this component's dirty mask");
        assert_eq!(d.code.display_code(), "G0201");
        assert!(d.render().starts_with("error[G0201]: slot reads `Cell<i32>`"));
        assert!(d.fixit.as_deref().unwrap().contains("Signal<i32>"));
    }

    /// Schema codes occupy the G01xx family in source order.
    #[test]
    fn schema_code_family() {
        assert_eq!(GsxCode::UnknownProp.display_code(), "G0101");
        assert_eq!(GsxCode::UnknownEvent.display_code(), "G0102");
        assert_eq!(GsxCode::ContentModel.display_code(), "G0103");
        assert_eq!(GsxCode::A11yRequired.display_code(), "G0104");
        assert_eq!(GsxCode::DuplicateAttr.display_code(), "G0105");
        assert_eq!(GsxCode::HandlerCapture.display_code(), "G0200");
        assert_eq!(GsxCode::StructuralWrite.display_code(), "G0202");
    }
}
