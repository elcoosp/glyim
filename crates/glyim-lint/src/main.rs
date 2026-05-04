use std::collections::HashMap;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: glyim-lint <file.g>");
        std::process::exit(1);
    }
    let input = PathBuf::from(&args[1]);
    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {}: {}", input.display(), e);
            std::process::exit(1);
        }
    };

    let parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        eprintln!("parse errors encountered, aborting lint");
        std::process::exit(1);
    }

    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);

    // Rule: check for functions without parameters that are never called
    let mut fn_counts: HashMap<glyim_interner::Symbol, usize> = HashMap::new();
    for item in &hir.items {
        if let glyim_hir::HirItem::Fn(f) = item
            && f.params.is_empty()
        {
            fn_counts.entry(f.name).or_insert(0);
        }
    }

    let mut found = 0;
    for item in &hir.items {
        if let glyim_hir::HirItem::Fn(f) = item
            && f.params.is_empty()
            && !fn_counts.contains_key(&f.name)
        {
            eprintln!(
                "warning: function '{}' has no parameters and may be unused",
                interner.resolve(f.name)
            );
            found += 1;
        }
    }

    if found == 0 {
        println!("No lint warnings found.");
    }
    std::process::exit(0);
}
