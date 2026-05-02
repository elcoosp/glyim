use crate::dump;
use std::path::PathBuf;

pub fn cmd_dump_tokens(input: PathBuf) -> i32 {
    let source = std::fs::read_to_string(&input).unwrap_or_default();
    dump::dump_tokens(&source, &mut std::io::stdout());
    0
}
