use std::fs;
use std::path::Path;
use glyim_orchestrator::orchestrator::{
    PackageGraphOrchestrator,
    OrchestratorConfig,
};

/// Write a minimal glyim.toml
fn write_manifest(dir: &Path, name: &str, deps: &[&str]) {
    let mut deps_toml = String::new();
    for dep in deps {
        deps_toml.push_str(&format!("{} = {{ version = \"*\" }}\n", dep));
    }
    let content = if deps.is_empty() {
        format!("[package]\nname = \"{}\"\nversion = \"0.1.0\"\n", name)
    } else {
        format!("[package]\nname = \"{}\"\nversion = \"0.1.0\"\n[dependencies]\n{}", name, deps_toml)
    };
    fs::write(dir.join("glyim.toml"), content).unwrap();
}

/// Write a trivial main.g file returning 42
fn write_lib_g(dir: &Path) {
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("main.g"), "pub fn answer() -> i64 { 42 }\n").unwrap();
}

fn write_main_g(dir: &Path) {
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("main.g"), "main = () => 42").unwrap();
}

/// Create a simple workspace with one package
#[test]
fn single_package_workspace_build() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Workspace manifest
    fs::write(root.join("glyim.toml"),
        "[workspace]\nmembers = [\"pkg\"]\n").unwrap();

    let pkg_dir = root.join("pkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    write_manifest(&pkg_dir, "pkg", &[]);
    write_main_g(&pkg_dir);

    let config = OrchestratorConfig::default();
    let mut orch = PackageGraphOrchestrator::new(root, config).unwrap();
    let result = orch.build();
    assert!(result.is_ok(), "build failed: {:?}", result.err());
    let output = result.unwrap();
    assert!(output.exists());
    let report = orch.report();
    assert!(report.packages_failed.is_empty());
    assert_eq!(report.packages_compiled.len(), 1);
}

/// Create a workspace with two packages where B depends on A
#[test]
fn two_package_workspace_build() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    fs::write(root.join("glyim.toml"),
        "[workspace]\nmembers = [\"a\", \"b\"]\n").unwrap();

    let a_dir = root.join("a");
    std::fs::create_dir_all(&a_dir).unwrap();
    write_manifest(&a_dir, "a", &[]);
    write_lib_g(&a_dir);

    let b_dir = root.join("b");
    std::fs::create_dir_all(&b_dir).unwrap();
    write_manifest(&b_dir, "b", &["a"]);
    // main uses a function from a: just call directly? We'll keep a dummy main
    fs::create_dir_all(b_dir.join("src")).unwrap();
    fs::write(b_dir.join("src/main.g"), "main = () => 0").unwrap();

    let config = OrchestratorConfig::default();
    let mut orch = PackageGraphOrchestrator::new(root, config).unwrap();
    let result = orch.build();
    assert!(result.is_ok(), "build failed: {:?}", result.err());
    let report = orch.report();
    assert!(report.packages_failed.is_empty());
    assert_eq!(report.packages_compiled.len(), 2);
    // Ensure 'a' compiled before 'b'
    let a_idx = report.packages_compiled.iter().position(|p| p == "a").unwrap();
    let b_idx = report.packages_compiled.iter().position(|p| p == "b").unwrap();
    assert!(a_idx < b_idx, "dependency order wrong");
}

/// Verify that a rebuild without changes uses cached artifacts
#[test]
fn incremental_build_uses_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    fs::write(root.join("glyim.toml"),
        "[workspace]\nmembers = [\"pkg\"]\n").unwrap();
    let pkg_dir = root.join("pkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    write_manifest(&pkg_dir, "pkg", &[]);
    write_main_g(&pkg_dir);

    // First build
    let config = OrchestratorConfig::default();
    let mut orch = PackageGraphOrchestrator::new(root, config.clone()).unwrap();
    orch.build().unwrap();
    let first_report = orch.report().clone();
    assert_eq!(first_report.packages_compiled.len(), 1);

    // Second build without changes
    let mut orch2 = PackageGraphOrchestrator::new(root, config).unwrap();
    orch2.build().unwrap();
    let second_report = orch2.report().clone();
    assert_eq!(second_report.packages_cached.len(), 1, "Should have used cache for pkg");
}

/// Verify that a change in a dependency triggers rebuild of dependent
#[test]
fn change_in_dep_triggers_rebuild_of_dependent() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    fs::write(root.join("glyim.toml"),
        "[workspace]\nmembers = [\"a\", \"b\"]\n").unwrap();

    let a_dir = root.join("a");
    std::fs::create_dir_all(&a_dir).unwrap();
    write_manifest(&a_dir, "a", &[]);
    write_lib_g(&a_dir);

    let b_dir = root.join("b");
    std::fs::create_dir_all(&b_dir).unwrap();
    write_manifest(&b_dir, "b", &["a"]);
    fs::create_dir_all(b_dir.join("src")).unwrap();
    fs::write(b_dir.join("src/main.g"), "main = () => 0").unwrap();

    // First build
    let config = OrchestratorConfig::default();
    let mut orch = PackageGraphOrchestrator::new(root, config.clone()).unwrap();
    orch.build().unwrap();

    // Modify a's source
    fs::write(a_dir.join("src/main.g"), "pub fn answer() -> i64 { 99 }\n").unwrap();

    // Second build
    let mut orch2 = PackageGraphOrchestrator::new(root, config).unwrap();
    orch2.build().unwrap();
    let report = orch2.report().clone();
    // a should be compiled, b should be compiled (since b depends on a)
    assert!(report.packages_compiled.contains(&"a".to_string()));
    assert!(report.packages_compiled.contains(&"b".to_string()));
}
