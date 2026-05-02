use super::*;

pub fn cmd_export(name: String, dest: PathBuf) -> i32 {
    eprintln!(
        "error: 'export' not implemented (artifact: {name}, dest: {})",
        dest.display()
    );
    1
}
