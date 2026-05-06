use glyim_compiler::pipeline::{self, BuildMode};
use std::path::PathBuf;

pub fn cmd_run(input: PathBuf, target: Option<String>, release: bool, live: bool, incremental: bool) -> i32 {
    let mode = if release {
        BuildMode::Release
    } else {
        BuildMode::Debug
    };
    if incremental {
        let source = match std::fs::read_to_string(&input) {
            Ok(s) => s,
            Err(e) => { eprintln!("error reading {}: {}", input.display(), e); return 1; }
        };
        return match pipeline::run_live(&source) {  // live is also incremental-friendly
            Ok(code) => code,
            Err(e) => { eprintln!("error: {e}"); 1 }
        };
    }
    if live {
        let source = match std::fs::read_to_string(&input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error reading {}: {}", input.display(), e);
                return 1;
            }
        };
        return match pipeline::run_live(&source) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        };
    }
    let result = if input.is_dir() {
        pipeline::run_package(&input, mode, target.as_deref())
    } else {
        pipeline::run_with_mode(&input, mode, target.as_deref(), None)
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
