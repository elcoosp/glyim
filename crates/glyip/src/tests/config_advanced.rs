//! Advanced configuration tests — edge cases and detailed dep specs.

use crate::config::*;
use std::collections::BTreeMap;

#[test]
fn parse_empty_dependencies() {
    let toml = r#"
[package]
name = "no-deps"
version = "0.1.0"
edition = "2024"

[dependencies]
[dev_dependencies]
"#;
    let config = GlyipToml::parse(toml).expect("parse");
    assert!(config.dependencies.is_empty());
    assert!(config.dev_dependencies.is_empty());
}

#[test]
fn parse_git_dependency() {
    let toml = r#"
[package]
name = "git-dep-proj"

[dependencies]
my-git = { git = "https://github.com/example/my-git", branch = "main" }
"#;
    let config = GlyipToml::parse(toml).expect("parse");
    let dep = config.dependencies.get("my-git").expect("git dep");
    match dep {
        Dependency::Detailed(d) => {
            assert_eq!(d.git.as_deref(), Some("https://github.com/example/my-git"));
            assert_eq!(d.branch.as_deref(), Some("main"));
            assert!(d.path.is_none());
        }
        _ => panic!("expected Detailed dependency"),
    }
}

#[test]
fn parse_git_dep_with_tag() {
    let toml = r#"
[package]
name = "tag-dep-proj"

[dependencies]
tagged = { git = "https://github.com/example/tagged", tag = "v1.0.0" }
"#;
    let config = GlyipToml::parse(toml).expect("parse");
    let dep = config.dependencies.get("tagged").expect("tagged dep");
    match dep {
        Dependency::Detailed(d) => {
            assert_eq!(d.tag.as_deref(), Some("v1.0.0"));
        }
        _ => panic!("expected Detailed dependency"),
    }
}

#[test]
fn parse_git_dep_with_rev() {
    let toml = r#"
[package]
name = "rev-dep-proj"

[dependencies]
pinned = { git = "https://github.com/example/pinned", rev = "abc123" }
"#;
    let config = GlyipToml::parse(toml).expect("parse");
    let dep = config.dependencies.get("pinned").expect("pinned dep");
    match dep {
        Dependency::Detailed(d) => {
            assert_eq!(d.rev.as_deref(), Some("abc123"));
        }
        _ => panic!("expected Detailed dependency"),
    }
}

#[test]
fn parse_path_dependency() {
    let toml = r#"
[package]
name = "path-dep-proj"

[dependencies]
local = { path = "../local-crate" }
"#;
    let config = GlyipToml::parse(toml).expect("parse");
    let dep = config.dependencies.get("local").expect("local dep");
    match dep {
        Dependency::Detailed(d) => {
            assert_eq!(
                d.path.as_deref(),
                Some(std::path::Path::new("../local-crate"))
            );
            assert!(d.version.is_none());
        }
        _ => panic!("expected Detailed dependency"),
    }
}

#[test]
fn parse_mixed_dependencies() {
    let toml = r#"
[package]
name = "mixed-deps"

[dependencies]
simple = "1.0"
detailed = { version = "2.0", path = "../detailed" }

[dev_dependencies]
dev-simple = "0.1"
"#;
    let config = GlyipToml::parse(toml).expect("parse");
    assert_eq!(config.dependencies.len(), 2);
    assert_eq!(config.dev_dependencies.len(), 1);

    // Simple dep
    let simple = config.dependencies.get("simple").expect("simple");
    assert!(matches!(simple, Dependency::Simple(_)));

    // Detailed dep
    let detailed = config.dependencies.get("detailed").expect("detailed");
    assert!(matches!(detailed, Dependency::Detailed(_)));
}

#[test]
fn parse_binary_and_library_targets() {
    let toml = r#"
[package]
name = "dual-target"

[[package.bin]]
name = "mybin"
path = "src/main.g"

[package.lib]
path = "src/lib.g"
"#;
    let config = GlyipToml::parse(toml).expect("parse");
    let bins = config.package.bin.as_ref().expect("bins");
    assert_eq!(bins.len(), 1);
    assert_eq!(bins[0].name, "mybin");

    let lib = config.package.lib.as_ref().expect("lib");
    assert_eq!(lib.path.as_deref(), Some(std::path::Path::new("src/lib.g")));
}

#[test]
fn read_from_nonexistent_dir() {
    let result = GlyipToml::read_from_dir(std::path::Path::new("/nonexistent/path/xyz"));
    assert!(result.is_err());
}

#[test]
fn write_and_read_config() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let config = GlyipToml {
        package: PackageConfig {
            name: "roundtrip2".to_string(),
            version: "3.0.0".to_string(),
            edition: "2024".to_string(),
            authors: vec!["Author".to_string()],
            description: Some("A test".to_string()),
            bin: None,
            lib: Some(LibTarget {
                path: Some(std::path::PathBuf::from("src/lib.g")),
            }),
        },
        dependencies: {
            let mut m = BTreeMap::new();
            m.insert("dep".to_string(), Dependency::Simple("1.0".to_string()));
            m
        },
        dev_dependencies: BTreeMap::new(),
    };

    config.write_to_dir(dir.path()).expect("write");
    let loaded = GlyipToml::read_from_dir(dir.path()).expect("read");
    assert_eq!(config, loaded);
}

#[test]
fn name_accessor() {
    let config = GlyipToml {
        package: PackageConfig {
            name: "accessor-test".to_string(),
            version: "0.1.0".to_string(),
            edition: "2024".to_string(),
            authors: Vec::new(),
            description: None,
            bin: None,
            lib: None,
        },
        dependencies: BTreeMap::new(),
        dev_dependencies: BTreeMap::new(),
    };
    assert_eq!(config.name(), "accessor-test");
}

#[test]
fn dependency_path_accessor() {
    let simple = Dependency::Simple("1.0".to_string());
    assert!(simple.path().is_none());
    assert!(simple.git().is_none());

    let with_path = Dependency::Detailed(DependencyDetail {
        version: None,
        path: Some(std::path::PathBuf::from("../local")),
        git: None,
        branch: None,
        tag: None,
        rev: None,
    });
    assert!(with_path.path().is_some());
    assert!(with_path.git().is_none());
}

#[test]
fn default_build_options() {
    let opts = BuildOptions::default();
    assert!(!opts.release);
    assert!(opts.target.is_none());
    assert_eq!(opts.backend, "llvm");
    assert_eq!(opts.opt_level, 0);
}

#[test]
fn default_run_options() {
    let opts = RunOptions::default();
    assert!(!opts.release);
    assert!(opts.args.is_empty());
    // `run` needs something the OS can exec.
    assert_eq!(opts.backend, "llvm");
}

#[test]
fn default_new_options() {
    let opts = NewOptions::default();
    assert!(!opts.lib);
    assert_eq!(opts.edition, "2024");
}
