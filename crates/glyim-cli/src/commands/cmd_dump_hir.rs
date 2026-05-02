use super::*;

pub fn cmd_dump_hir(input: PathBuf) -> i32 {
    let source = std::fs::read_to_string(&input).unwrap_or_default();
    let interner = glyim_interner::Interner::new();
    glyim_cli::dump::dump_hir(&source, &interner, &mut std::io::stdout());
    0
}
