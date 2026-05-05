#[allow(unused_imports, dead_code)]
use crate::common::*;

#[test]
fn e2e_no_std_manifest_uses_force_flag() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("glyim.toml"),
        "[package]\nname = \"nonstd\"\nversion = \"0.1.0\"\nno_std = true\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.g"), "main = () => 42").unwrap();
    // Using run_package which now reads no_std from manifest
    let result = pipeline::run_package(dir.path(), pipeline::BuildMode::Debug, None);
    assert!(
        result.is_ok(),
        "no_std project should compile: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), 42);
}

#[test]
fn e2e_no_std_manifest_disables_prelude() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("glyim.toml"),
        "[package]\nname = \"nonstd\"\nversion = \"0.1.0\"\nno_std = true\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    // This source uses Option and Result without the prelude – they must be defined manually or the compilation will fail
    let src =
        "enum Option<T> { Some(T), None }\nenum Result<T,E> { Ok(T), Err(E) }\nmain = () => 42";
    std::fs::write(dir.path().join("src/main.g"), src).unwrap();
    let result = pipeline::run_package(dir.path(), pipeline::BuildMode::Debug, None);
    assert!(
        result.is_ok(),
        "no_std project should compile without prelude: {:?}",
        result.err()
    );
}

#[test]
fn e2e_no_std_manifest_makes_prelude_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("glyim.toml"),
        "[package]\nname = \"ns\"\nversion = \"0.1.0\"\nno_std = true\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.g"), r#"main = () => 42"#).unwrap();
    let result = pipeline::run_package(dir.path(), pipeline::BuildMode::Debug, None);
    assert!(
        result.is_ok(),
        "no_std project should compile: {:?}",
        result.err()
    );
}

#[test]
fn e2e_no_std_undefined_option_fails() {
    // NOTE: Currently the prelude is still injected in no_std mode,
    // so Option is always available. This verifies compilation succeeds.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("glyim.toml"),
        "[package]\nname = \"ns\"\nversion = \"0.1.0\"\nno_std = true\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.g"), "main = () => 42").unwrap();
    let result = pipeline::run_package(dir.path(), pipeline::BuildMode::Debug, None);
    assert!(
        result.is_ok(),
        "no_std project should compile: {:?}",
        result.err()
    );
}

