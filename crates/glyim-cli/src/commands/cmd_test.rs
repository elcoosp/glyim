use crate::pipeline;
use std::path::PathBuf;

pub fn cmd_test(input: PathBuf, ignore: bool, filter: Option<String>, nocapture: bool) -> i32 {
    let include_ignored = ignore;
    if nocapture {
        eprintln!("note: --nocapture flag is not yet fully implemented");
    }
    let result = if input.is_dir() {
        pipeline::run_tests_package(&input, filter.as_deref(), include_ignored)
    } else {
        pipeline::run_tests(&input, filter.as_deref(), include_ignored, None)
    };
    match result {
        Ok(summary) => {
            eprintln!("{}", summary.format_summary());
            summary.exit_code()
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
