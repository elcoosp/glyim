use glyim_lint::{LintRegistry, lint};
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: glyim-lint <file.g>");
        process::exit(1);
    }
    let input = &args[1];
    let source = fs::read_to_string(input).unwrap_or_else(|e| {
        eprintln!("error reading {}: {}", input, e);
        process::exit(1);
    });

    let parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        eprintln!("parse errors encountered, aborting lint");
        process::exit(1);
    }

    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);

    let registry = LintRegistry::new();
    let diagnostics = lint(&hir, &interner, &registry);

    if diagnostics.is_empty() {
        println!("No lint warnings found.");
    } else {
        for diag in &diagnostics {
            eprintln!(
                "{:?}: {} ({:?}): {}",
                diag.severity, diag.lint_id.0, diag.span, diag.message
            );
        }
        process::exit(1);
    }

    process::exit(0);
}
