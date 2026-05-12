use crate::artifact::CompiledArtifact;
use crate::collector::collect_tests;
use crate::harness;

pub struct Compiler;

impl Compiler {
    pub fn compile(source: &str, filter: Option<&str>) -> Result<CompiledArtifact, CompileError> {
        Self::compile_with_opts(source, filter, false)
    }

    pub fn compile_with_opts(
        source: &str,
        filter: Option<&str>,
        coverage: bool,
    ) -> Result<CompiledArtifact, CompileError> {
        let parse_out = glyim_parse::parse(source);
        if !parse_out.errors.is_empty() {
            return Err(CompileError::Parse(
                parse_out
                    .errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        let interner = &parse_out.interner;
        let test_defs = collect_tests(&parse_out.ast, interner, filter, false);

        if test_defs.is_empty() {
            return Err(CompileError::NoTests);
        }

        let tmp_dir = tempfile::tempdir().map_err(CompileError::Io)?;

        let prelude = "\npub enum Option<T> {\n    Some(T),\n    None,\n}\npub enum Result<T, E> {\n    Ok(T),\n    Err(E),\n}\nextern {\n    fn __glyim_alloc(size: i64) -> *mut u8;\n    fn __glyim_free(ptr: *mut u8);\n    fn __glyim_hash_bytes(data: *const u8, len: i64) -> i64;\n    fn __glyim_hash_seed() -> i64;\n    fn abort();\n}\n";
        if test_defs.len() == 1 {
            let test_name = &test_defs[0].name;
            let body = harness::inject_single_test(source, test_name);
            let full_source = format!("{}\n{}", prelude, body);
            let bin = tmp_dir.path().join("test_bin");
            glyim_compiler::pipeline::build_raw(
                &full_source,
                &bin,
                glyim_compiler::BuildMode::Debug,
            )
            .map_err(|e| CompileError::Pipeline(format!("{:?}", e)))?;
            return Ok(CompiledArtifact {
                test_defs,
                bin_path: Some(bin),
                per_test_binaries: vec![],
                _temp_dir: tmp_dir,
            });
        }

        // Multiple tests: compile each as separate binary
        let prelude = glyim_compiler::pipeline::PRELUDE; // if PRELUDE is not pub, we may need to copy it. We'll include it from the const in pipeline.
        // Actually PRELUDE is private. We'll just inline it here.
        let prelude = "\npub enum Option<T> {\n    Some(T),\n    None,\n}\npub enum Result<T, E> {\n    Ok(T),\n    Err(E),\n}\nextern {\n    fn __glyim_alloc(size: i64) -> *mut u8;\n    fn __glyim_free(ptr: *mut u8);\n    fn __glyim_hash_bytes(data: *const u8, len: i64) -> i64;\n    fn __glyim_hash_seed() -> i64;\n    fn abort();\n}\n";
        let mut per_test_binaries: Vec<(String, std::path::PathBuf)> = Vec::new();
        for test_def in &test_defs {
            let body = harness::inject_single_test(source, &test_def.name);
            let full_source = format!("{}\n{}", prelude, body);
            let bin = tmp_dir.path().join(&test_def.name);
            glyim_compiler::pipeline::build_raw(
                &full_source,
                &bin,
                glyim_compiler::BuildMode::Debug,
            )
            .map_err(|e| CompileError::Pipeline(format!("{:?}", e)))?;
            per_test_binaries.push((test_def.name.clone(), bin));
        }

        Ok(CompiledArtifact {
            test_defs,
            bin_path: None,
            per_test_binaries,
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
