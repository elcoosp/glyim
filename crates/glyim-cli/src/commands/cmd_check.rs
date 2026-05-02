use super::*;

pub fn cmd_check(input: PathBuf) -> i32 {
    match pipeline::check(&input) {
        Ok(()) => 0,
        Err(e) => { eprintln!("error: {e}"); 1 }
    }
}
