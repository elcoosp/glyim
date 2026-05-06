use std::env;
use std::fs;
use std::process;
use glyim_fmt::{format_source, FormatConfig};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: glyim-fmt [--check] <file.g>");
        process::exit(1);
    }
    let check_mode = args[1] == "--check";
    let input = if check_mode { &args[2] } else { &args[1] };

    let source = fs::read_to_string(input).unwrap_or_else(|e| {
        eprintln!("error reading {}: {}", input, e);
        process::exit(1);
    });

    let config = FormatConfig::default();
    let formatted = format_source(&source, &config).unwrap_or_else(|e| {
        eprintln!("{}: formatting error: {}", input, e);
        process::exit(1);
    });

    if check_mode {
        if formatted != source {
            eprintln!("{}: formatting required", input);
            process::exit(1);
        }
    } else {
        print!("{}", formatted);
    }
}
