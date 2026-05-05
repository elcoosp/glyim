use glyim_compiler::pipeline::{self, BuildMode};
use std::path::PathBuf;

pub fn cmd_run(input: PathBuf, target: Option<String>, release: bool) -> i32 {
    let mode = if release {
        BuildMode::Release
    } else {
        BuildMode::Debug
    };
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
