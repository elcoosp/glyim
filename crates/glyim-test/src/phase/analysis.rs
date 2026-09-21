use crate::mock::TestDbBuilder;
use std::path::PathBuf;
use std::sync::Arc;

/// AnalysisTester.
pub struct AnalysisTester {
    source: String,
}

impl AnalysisTester {
    /// new.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
        }
    }
    /// run_def_map.
    pub fn run_def_map(self) -> super::CompilationTrace {
        let mut trace = super::CompilationTrace::default();
        let parse = glyim_frontend::parse_to_syntax(&self.source, glyim_span::FileId::from_raw(0));
        trace.parse_diagnostics = parse.diagnostics;
        trace.parse_tree = Some(parse.root);
        let _db = TestDbBuilder::new()
            .name("analysis_test")
            .file(PathBuf::from("test.g"), Arc::from(self.source.as_str()))
            .build();
        trace
    }
}
