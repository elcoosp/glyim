use glyim_pkg::lockfile::{generate_lockfile, serialize_lockfile, LockSource};
use glyim_pkg::manifest::PackageManifest;
use glyim_pkg::resolver::{resolve, Requirement};
use std::collections::HashMap;
use std::path::Path;

use sha2::Digest;
/// Resolve dependencies from manifest and write glyim.lock.
/// Returns Ok even if resolution fails partially (e.g., registry unavailable).
pub fn resolve_and_write_lockfile(
    package_dir: &Path,
    manifest: &PackageManifest,
) -> Result<(), String> {
    let mut requirements: Vec<Requirement> = Vec::new();
    for (name, dep) in &manifest.dependencies {
        let version = dep.version.as_deref().unwrap_or("*");
        let source = if let Some(ref path) = dep.path {
            LockSource::Path {
                path: path.to_string_lossy().to_string(),
            }
        } else if let Some(ref registry) = dep.registry {
            LockSource::Registry {
                url: registry.clone(),
            }
        } else {
            LockSource::Registry {
                url: "https://registry.glyim.dev".to_string(),
            }
        };
        requirements.push(Requirement {
            name: name.clone(),
            version_constraint: version.to_string(),
            is_macro: false,
            source,
        });
    }
    for (name, dep) in &manifest.macros {
        let version = dep.version.as_deref().unwrap_or("*");
        let source = if let Some(ref path) = dep.path {
            LockSource::Path {
                path: path.to_string_lossy().to_string(),
            }
        } else if let Some(ref registry) = dep.registry {
            LockSource::Registry {
                url: registry.clone(),
            }
        } else {
            LockSource::Registry {
                url: "https://registry.glyim.dev".to_string(),
            }
        };
        requirements.push(Requirement {
            name: name.clone(),
            version_constraint: version.to_string(),
            is_macro: true,
            source,
        });
    }

    let lockfile_path = package_dir.join("glyim.lock");
    if requirements.is_empty() {
        let lockfile = generate_lockfile(&HashMap::new());
        let serialized = serialize_lockfile(&lockfile);
        std::fs::write(&lockfile_path, serialized).map_err(|e| format!("write lockfile: {e}"))?;
        return Ok(());
    }

    let existing_lockfile = if lockfile_path.exists() {
        let content =
            std::fs::read_to_string(&lockfile_path).map_err(|e| format!("read lockfile: {e}"))?;
        glyim_pkg::lockfile::parse_lockfile(&content).ok()
    } else {
        None
    };

    let mut available: HashMap<String, Vec<glyim_pkg::resolver::AvailableVersion>> = HashMap::new();
    for req in &requirements {
        if let LockSource::Path { ref path } = req.source {
            let abs_path = package_dir.join(path);
            if abs_path.join("glyim.toml").exists() {
                let _hash = compute_path_hash(&abs_path)?;
                available.entry(req.name.clone()).or_default().push(
                    glyim_pkg::resolver::AvailableVersion {
                        version: "0.0.0".to_string(),
                        is_macro: req.is_macro,
                        deps: vec![],
                        source: req.source.clone(),
                    },
                );
            }
        }
    }

    let resolution = resolve(&requirements, existing_lockfile.as_ref(), &available)
        .map_err(|e| format!("resolve: {e}"))?;

    let mut resolved_map: HashMap<String, (String, String, bool, Vec<String>, LockSource)> =
        HashMap::new();
    for (name, pkg) in &resolution.packages {
        resolved_map.insert(
            name.clone(),
            (
                pkg.version.clone(),
                format!("sha256:{}", hex::encode([0u8; 32])),
                pkg.is_macro,
                pkg.deps.clone(),
                pkg.source.clone(),
            ),
        );
    }

    let lockfile = generate_lockfile(&resolved_map);
    let serialized = serialize_lockfile(&lockfile);
    std::fs::write(&lockfile_path, serialized).map_err(|e| format!("write lockfile: {e}"))?;
    eprintln!(
        "Generated glyim.lock ({} packages)",
        resolution.packages.len()
    );
    Ok(())
}

fn compute_path_hash(path: &std::path::Path) -> Result<String, String> {
    let mut hasher = sha2::Sha256::new();
    walk_dir_for_hash(path, &mut hasher).map_err(|e| format!("hash path: {e}"))?;
    Ok(hex::encode(hasher.finalize()))
}

fn walk_dir_for_hash(path: &std::path::Path, hasher: &mut sha2::Sha256) -> Result<(), String> {
    if path.is_file() {
        let content = std::fs::read(path).map_err(|e| format!("read {path:?}: {e}"))?;
        hasher.update(&content);
        return Ok(());
    }
    if path.is_dir() {
        let entries = std::fs::read_dir(path).map_err(|e| format!("read dir {path:?}: {e}"))?;
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            walk_dir_for_hash(&entry.path(), hasher)?;
        }
    }
    Ok(())
}

use glyim_pkg::lockfile::{parse_lockfile, LockedPackage};

/// Read packages from glyim.lock in the given directory.
pub fn read_lockfile_packages(package_dir: &Path) -> Result<Vec<LockedPackage>, String> {
    let lockfile_path = package_dir.join("glyim.lock");
    if !lockfile_path.exists() {
        return Ok(vec![]);
    }
    let content =
        std::fs::read_to_string(&lockfile_path).map_err(|e| format!("read lockfile: {e}"))?;
    let lockfile = parse_lockfile(&content).map_err(|e| format!("parse lockfile: {e}"))?;
    Ok(lockfile.packages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockfile_written_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let lockfile_path = dir.path().join("glyim.lock");
        let mut resolved = HashMap::new();
        resolved.insert(
            "test-pkg".to_string(),
            (
                "1.0.0".to_string(),
                "sha256:abcdef".to_string(),
                false,
                vec![],
                glyim_pkg::lockfile::LockSource::Local,
            ),
        );
        let lock = generate_lockfile(&resolved);
        let serialized = serialize_lockfile(&lock);
        std::fs::write(&lockfile_path, &serialized).unwrap();
        let content = std::fs::read_to_string(&lockfile_path).unwrap();
        assert!(content.contains("test-pkg"));
        assert!(content.contains("@generated"));
    }

    #[test]
    fn resolve_empty_deps_writes_empty_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = PackageManifest {
            package: glyim_pkg::manifest::Package::default(),
            dependencies: HashMap::new(),
            macros: HashMap::new(),
            dev_dependencies: HashMap::new(),
            target: HashMap::new(),
            cache: None,
            features: glyim_pkg::manifest::FeaturesConfig::default(),
            workspace: None,
        };
        resolve_and_write_lockfile(dir.path(), &manifest).unwrap();
        let content = std::fs::read_to_string(dir.path().join("glyim.lock")).unwrap();
        assert!(content.contains("@generated"));
    }
}
