use glyim_compiler::pipeline;
use std::path::PathBuf;

pub fn cmd_check(input: PathBuf, incremental: bool) -> i32 {
    if incremental {
        let source = match std::fs::read_to_string(&input) {
            Ok(s) => s,
            Err(e) => { eprintln!("error reading {}: {}", input.display(), e); return 1; }
        };
        use glyim_compiler::queries::QueryPipeline;
        let cache_dir = input.parent().unwrap_or(std::path::Path::new(".")).join(".glyim/incremental");
        let mut qp = QueryPipeline::new(&cache_dir, Default::default());
        return match qp.compile(&source, &input) {
            Ok(_) => { eprintln!("Check passed (incremental)"); 0 },
            Err(e) => { eprintln!("error: {e}"); 1 }
        };
    }
    if incremental {
        let source = match std::fs::read_to_string(&input) {
            Ok(s) => s,
            Err(e) => { eprintln!("error reading {}: {}", input.display(), e); return 1; }
        };
        use glyim_compiler::queries::QueryPipeline;
        let cache_dir = input.parent().unwrap_or(std::path::Path::new(".")).join(".glyim/incremental");
        let mut qp = QueryPipeline::new(&cache_dir, Default::default());
        return match qp.compile(&source, &input) {
            Ok(_) => { eprintln!("Check passed (incremental)"); 0 },
            Err(e) => { eprintln!("error: {e}"); 1 }
        };
    }
    match pipeline::check(&input) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
