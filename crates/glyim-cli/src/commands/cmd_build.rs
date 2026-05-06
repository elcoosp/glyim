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
    if incremental {
        eprintln!("Using incremental compilation pipeline...");
        return match pipeline::build_incremental(&input, output.as_deref(), mode, target.as_deref()) {
            Ok((_path, report)) => {
                eprintln!("Incremental build: {:?} ({:.1}ms)",
                    if report.was_full_rebuild { "full rebuild" } else { "incremental" },
                    report.total_elapsed.as_secs_f64() * 1000.0);
                0
            }
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        };
    }
    let result = if bare || input.is_file() {
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
