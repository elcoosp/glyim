use glyim_compiler::pipeline;
use std::path::PathBuf;

pub fn cmd_ir(input: PathBuf) -> i32 {
    match pipeline::print_ir(&input) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
