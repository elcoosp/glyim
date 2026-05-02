use std::path::PathBuf;
use crate::pipeline::{self, BuildMode};

pub fn cmd_run(
    input: PathBuf,
    target: Option<String>,
    _debug: bool,
    release: bool,
) -> i32 {
    let mode = if release {
        BuildMode::Release
    } else {
        BuildMode::Debug
    };
    let result = if input.is_dir() {
        pipeline::run_package(&input, mode, target.as_deref())
    } else {
        pipeline::run_with_mode(&input, mode, target.as_deref())
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
