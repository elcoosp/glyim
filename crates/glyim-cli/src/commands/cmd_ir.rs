use super::*;

pub fn cmd_ir(input: PathBuf) -> i32 {
    match pipeline::print_ir(&input) {
        Ok(()) => 0,
        Err(e) => { eprintln!("error: {e}"); 1 }
    }
}
