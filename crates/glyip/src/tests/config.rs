//! Tests for Glyip.toml parsing and serialisation.

use crate::config::*;
use std::collections::BTreeMap;

#[test]
fn parse_minimal_config() {
    let toml = r#"
[package]
name = "my-crate"
"#;
    let config = GlyipToml::parse(toml).expect("parse should succeed");
    assert_eq!(config.package.name, "my-crate");
    assert_eq!(config.package.version, "0.1.0");
    assert_eq!(config.package.edition, "2024");
    assert!(config.dependencies.is_empty());
    assert!(config.dev_dependencies.is_empty());
}

#[test]
fn parse_full_config() {
    let toml = r#"
[package]
name = "full-crate"
version = "2.0.0"
edition = "2024"
authors = ["Alice", "Bob"]
description = "A full-featured crate"

[dependencies]
serde = "1.0"
my-local = { path = "../my-local" }

[dev_dependencies]
test-utils = { version = "0.1", path = "../test-utils" }
"#;
    let config = GlyipToml::parse(toml).expect("parse should succeed");
    assert_eq!(config.package.name, "full-crate");
    assert_eq!(config.package.version, "2.0.0");
    assert_eq!(config.package.authors, vec!["Alice", "Bob"]);
    assert_eq!(config.dependencies.len(), 2);
    assert_eq!(config.dev_dependencies.len(), 1);

    let serde_dep = config.dependencies.get("serde").expect("serde dep");
    assert_eq!(serde_dep.version(), Some("1.0"));
    assert!(serde_dep.path().is_none());

    let local_dep = config.dependencies.get("my-local").expect("local dep");
    assert!(local_dep.path().is_some());
}

#[test]
fn parse_invalid_toml() {
    let result = GlyipToml::parse("this is not valid toml {{{}}");
    assert!(result.is_err());
}

#[test]
fn roundtrip_config() {
    let config = GlyipToml {
        package: PackageConfig {
            name: "roundtrip".to_string(),
            version: "1.0.0".to_string(),
            edition: "2024".to_string(),
            authors: vec!["Tester".to_string()],
            description: Some("Test crate".to_string()),
            bin: Some(vec![BinTarget {
                name: "roundtrip".to_string(),
                path: Some(std::path::PathBuf::from("src/main.g")),
            }]),
            lib: None,
        },
        dependencies: {
            let mut m = BTreeMap::new();
            m.insert("dep1".to_string(), Dependency::Simple("1.0".to_string()));
            m
        },
        dev_dependencies: BTreeMap::new(),
    };

    let serialized = toml::to_string_pretty(&config).expect("serialize");
    let reparsed = GlyipToml::parse(&serialized).expect("re-parse");
    assert_eq!(config, reparsed);
}

#[test]
fn dependency_accessors() {
    let simple = Dependency::Simple("1.0".to_string());
    assert_eq!(simple.version(), Some("1.0"));
    assert!(simple.path().is_none());
    assert!(simple.git().is_none());

    let detailed = Dependency::Detailed(DependencyDetail {
        version: Some("2.0".to_string()),
        path: Some(std::path::PathBuf::from("../local")),
        git: Some("https://example.com/repo".to_string()),
        branch: Some("main".to_string()),
        tag: None,
        rev: None,
    });
    assert_eq!(detailed.version(), Some("2.0"));
    assert!(detailed.path().is_some());
    assert!(detailed.git().is_some());
}

#[test]
fn all_dependencies_iter() {
    let config = GlyipToml {
        package: PackageConfig {
            name: "test".to_string(),
            version: "0.1.0".to_string(),
            edition: "2024".to_string(),
            authors: Vec::new(),
            description: None,
            bin: None,
            lib: None,
        },
        dependencies: {
            let mut m = BTreeMap::new();
            m.insert("dep1".to_string(), Dependency::Simple("1.0".to_string()));
            m
        },
        dev_dependencies: {
            let mut m = BTreeMap::new();
            m.insert("dev1".to_string(), Dependency::Simple("0.1".to_string()));
            m
        },
    };

    let all: Vec<_> = config.all_dependencies().collect();
    assert_eq!(all.len(), 2);
}

#[test]
fn default_options() {
    let new_opts = NewOptions::default();
    assert!(!new_opts.lib);
    assert_eq!(new_opts.edition, "2024");

    let build_opts = BuildOptions::default();
    assert!(!build_opts.release);
    assert_eq!(build_opts.backend, "llvm");

    let test_opts = TestOptions::default();
    assert!(test_opts.filter.is_none());
    assert!(!test_opts.no_run);

    let run_opts = RunOptions::default();
    assert!(run_opts.args.is_empty());
}
