use glyim_parse::parse;
use glyim_hir::lower;
use glyim_typeck::TypeChecker;
use glyim_interner::Interner;
use std::path::PathBuf;


fn temp_g(content: &str) -> PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.g");
    std::fs::write(&path, content).unwrap();
    Box::leak(Box::new(dir));
    path
}
