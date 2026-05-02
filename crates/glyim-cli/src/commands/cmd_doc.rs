use std::path::PathBuf;
use glyim_parse;
use glyim_hir;
use glyim_doc;

pub fn cmd_doc(input: PathBuf, output: Option<PathBuf>) -> i32 {
    let source = std::fs::read_to_string(&input).unwrap_or_default();
    let parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        eprintln!("parse errors: {:?}", parse_out.errors);
        1
    } else {
        let mut interner = parse_out.interner;
        let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
        let html = glyim_doc::generate_html(&hir, &interner);
        let out_path = output.as_deref().unwrap_or(std::path::Path::new("doc/index.html"));
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Err(e) = std::fs::write(out_path, html) { eprintln!("error: {e}"); 1 } else { 0 }
    }
}
