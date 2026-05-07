use glyim_fmt::{format_source, FormatConfig};
use std::path::PathBuf;
use std::fs;

pub fn cmd_fmt(input: PathBuf, check: bool) -> i32 {
    let source = match fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {}: {}", input.display(), e);
            return 1;
        }
    };

    let config = FormatConfig::default();
    let formatted = match format_source(&source, &config) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error formatting {}: {:?}", input.display(), e);
            return 1;
        }
    };

    if check {
        if formatted != source {
            eprintln!("{}: formatting required", input.display());
            return 1;
        }
    } else {
        // Write back to file
        if let Err(e) = fs::write(&input, &formatted) {
            eprintln!("error writing {}: {}", input.display(), e);
            return 1;
        }
        eprintln!("Formatted {}", input.display());
    }
    0
}
