use crate::macro_expand;
use std::path::PathBuf;

pub fn cmd_macro_inspect(input: PathBuf) -> i32 {
    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {}: {}", input.display(), e);
            return 1;
        }
    };
    let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
    let pkg_dir = input
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    match macro_expand::expand_macros(&source, pkg_dir, &cas_dir) {
        Ok(expanded) => {
            println!("Original:\n{}", source);
            println!("\n──────────────────────\n");
            println!("Expanded:\n{}", expanded);
            0
        }
        Err(e) => {
            eprintln!("macro expansion failed: {}", e);
            1
        }
    }
}
