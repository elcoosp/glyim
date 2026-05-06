use glyim_compiler::queries::QueryPipeline;
use std::path::PathBuf;

pub fn cmd_incremental_status(input: PathBuf) -> i32 {
    let cache_dir = input
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(".glyim/incremental");

    if !cache_dir.exists() {
        eprintln!("No incremental cache found at {}", cache_dir.display());
        eprintln!("Run 'glyim build --incremental' to create one.");
        return 1;
    }

    let qp = QueryPipeline::new(&cache_dir, Default::default());
    let total = qp.ctx().len();

    eprintln!("Incremental cache statistics:");
    eprintln!("  Cache directory: {}", cache_dir.display());
    eprintln!("  Total cached queries: {}", total);

    if total == 0 {
        eprintln!("  No cached queries yet. Run a build first.");
    }

    0
}
