use glyim_pkg::manifest::parse_manifest;

#[test]
fn parse_valid_minimal_manifest() {
    let toml = r#"
[package]
name = "hello"
version = "0.1.0"
"#;
    let m = parse_manifest(toml, "glyim.toml").unwrap();
    assert_eq!(m.package.name, "hello");
    assert_eq!(m.package.version, "0.1.0");
}

#[test]
fn parse_with_dependencies() {
    let toml = r#"
[package]
name = "app"
version = "1.0.0"

[dependencies]
serde = { version = "1.0", path = "../serde", registry = "https://example.com" }
log = { version = "*" }
"#;
    let m = parse_manifest(toml, "glyim.toml").unwrap();
    assert_eq!(m.dependencies.len(), 2);
    assert_eq!(m.dependencies["serde"].version.as_deref(), Some("1.0"));
    assert_eq!(
        m.dependencies["serde"]
            .path
            .as_ref()
            .unwrap()
            .to_str()
            .unwrap(),
        "../serde"
    );
    assert!(m.dependencies["log"].is_macro == false);
}

#[test]
fn parse_workspace_only() {
    let toml = r#"
[workspace]
members = ["crates/*"]
"#;
    let m = parse_manifest(toml, "glyim.toml").unwrap();
    assert!(m.workspace.is_some());
    assert_eq!(m.workspace.unwrap().members, vec!["crates/*"]);
}

#[test]
fn parse_missing_package_name_fails() {
    let toml = r#"
[package]
version = "0.1.0"
"#;
    let res = parse_manifest(toml, "glyim.toml");
    assert!(res.is_err());
    let err = res.unwrap_err().to_string();
    assert!(err.contains("package.name") || err.contains("name"));
}

#[test]
fn parse_invalid_toml_fails() {
    let toml = "broken [";
    let res = parse_manifest(toml, "glyim.toml");
    assert!(res.is_err());
}

#[test]
fn parse_empty_manifest_fails() {
    let res = parse_manifest("", "glyim.toml");
    assert!(res.is_err());
}
