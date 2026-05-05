use crate::artifact::CompiledArtifact;
use crate::collector::collect_tests;
use crate::harness;

pub struct Compiler;

impl Compiler {
    pub fn compile(source: &str) -> Result<CompiledArtifact, CompileError> {
        let parse_out = glyim_parse::parse(source);
        if !parse_out.errors.is_empty() {
            return Err(CompileError::Parse(
                parse_out.errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", ")
            ));
        }
        let interner = &parse_out.interner;
        let test_defs = collect_tests(&parse_out.ast, interner, None, false);

        let test_names: Vec<String> = test_defs.iter().map(|t| t.name.clone()).collect();
        if test_names.is_empty() {
            return Err(CompileError::NoTests);
        }

        let modified_source = harness::inject_harness(source, &test_names);

        let tmp_dir = tempfile::tempdir().map_err(CompileError::Io)?;
        let source_path = tmp_dir.path().join("test.g");
        std::fs::write(&source_path, &modified_source).map_err(CompileError::Io)?;

        let bin_path = tmp_dir.path().join("test_bin");
        glyim_compiler::pipeline::build(&source_path, Some(&bin_path), None)
            .map_err(|e| CompileError::Pipeline(format!("{:?}", e)))?;

        Ok(CompiledArtifact {
            bin_path,
            test_defs,
            _temp_dir: tmp_dir,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("pipeline error: {0}")]
    Pipeline(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("no test functions found")]
    NoTests,
}
