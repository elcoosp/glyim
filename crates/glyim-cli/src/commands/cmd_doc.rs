use crate::pipeline;
use std::path::PathBuf;

pub fn cmd_doc(input: PathBuf, output: Option<PathBuf>) -> i32 {
    match pipeline::generate_doc(&input, output.as_deref()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
