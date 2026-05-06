use glyim_compiler::pipeline::{self, BuildMode};
use std::path::PathBuf;

pub fn cmd_build(
    input: PathBuf,
    output: Option<PathBuf>,
    target: Option<String>,
    release: bool,
    bare: bool,
    incremental: bool,  // NEW
) -> i32 {
    let mode = if release {
        BuildMode::Release
    } else {
        BuildMode::Debug
    };
    let result = if incremental {
        let cache_dir = dirs_next::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from(".glyim/cache"))
            .join("incremental");
        let (source, _) = pipeline::load_source_with_prelude(&input)?;
        let config = pipeline::PipelineConfig {
            mode,
            target,
            ..Default::default()
        };
        let compiled = pipeline::compile_source_to_hir_incremental(
            source, &input, &config, &cache_dir,
        )?;
        // Fall back to standard build for now (codegen + link not yet incremental)
        drop(compiled);
        pipeline::build_with_mode(&input, output.as_deref(), mode, target.as_deref(), None)
    } else if bare || input.is_file() {
        // Single file compilation (--bare or direct file input)
        pipeline::build_with_mode(&input, output.as_deref(), mode, target.as_deref(), None)
    } else {
        pipeline::build_package(&input, output.as_deref(), mode, target.as_deref())
    };
    match result {
        Ok(path) => {
            eprintln!("Built: {}", path.display());
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
