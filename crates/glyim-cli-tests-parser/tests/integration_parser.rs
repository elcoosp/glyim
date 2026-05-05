use glyim_parse::parse;
use glyim_hir::lower;
use glyim_typeck::TypeChecker;
use glyim_interner::Interner;
use std::path::PathBuf;

fn check(path: &PathBuf) -> Result<(), String> {
    let source = std::fs::read_to_string(path).unwrap();
    let parse_out = parse(&source);
    if !parse_out.errors.is_empty() {
        return Err(format!("parse: {:?}", parse_out.errors));
    }
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let mut tc = TypeChecker::new(interner);
    tc.check(&hir).map_err(|e| format!("typeck: {:?}", e))
}

fn temp_g(content: &str) -> PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.g");
    std::fs::write(&path, content).unwrap();
    Box::leak(Box::new(dir));
    path
}
