#[allow(unused_imports)]
pub use glyim_cli::pipeline;
pub use std::path::PathBuf;

pub fn temp_g(content: &str) -> PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.g");
    std::fs::write(&path, content).unwrap();
    Box::leak(Box::new(dir));
    path
}
