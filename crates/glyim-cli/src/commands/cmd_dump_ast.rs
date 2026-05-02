use super::*;

pub fn cmd_dump_ast(input: PathBuf) -> i32 {
    let source = std::fs::read_to_string(&input).unwrap_or_default();
    let parse_out = glyim_parse::parse(&source);
    let interner = parse_out.interner;
    glyim_cli::dump::dump_ast(&source, &interner, &mut std::io::stdout());
    if !parse_out.errors.is_empty() {
        for e in &parse_out.errors { eprintln!("error: {e}"); }
        1
    } else { 0 }
}
