use std::env;
use std::fs;
use std::process;

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

    // For now, re-emit the source unchanged.
    if check_mode {
        let tokens = glyim_lex::tokenize(&source);
        let mut formatted = String::new();
        for tok in &tokens {
            formatted.push_str(tok.text);
        }
        if formatted != source {
            eprintln!("{}: formatting required", input);
            process::exit(1);
        }
    } else {
        print!("{}", source);
    }
}
