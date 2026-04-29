use crate::error::PkgError;
use crate::lockfile::LockSource;
use std::collections::HashMap;

/// A version requirement from a dependency declaration.
pub struct Requirement {
    pub name: String,
    pub version_constraint: String,
    pub is_macro: bool,
    pub source: LockSource,
}

/// A resolved package with its selected version and transitive dependency names.
pub struct ResolvedPackage {
    pub version: String,
    pub is_macro: bool,
    pub deps: Vec<String>,
    pub source: LockSource,
}

/// Result of dependency resolution.
pub struct Resolution {
    pub packages: HashMap<String, ResolvedPackage>,
}

/// An available version from the registry.
pub struct AvailableVersion {
    pub version: String,
    pub is_macro: bool,
    pub deps: Vec<Requirement>,
    pub source: LockSource,
}

/// Check if a concrete version satisfies a constraint string.
///
/// Supports:
///   - Exact: "1.2.3" → true only for "1.2.3"
///   - Caret: "^1.2.3" → true for ">=1.2.3, <2.0.0"
///   - Wildcard: "*" → true for everything
pub fn satisfies_constraint(version: &str, constraint: &str) -> bool {
    if constraint == "*" {
        return true;
    }
    if version == constraint {
        return true;
    }
    if let Some(rest) = constraint.strip_prefix('^') {
        if let (Ok(ver), Ok(req)) = (semver::Version::parse(version), semver::Version::parse(rest)) {
            // ^1.2.3 means >=1.2.3, <2.0.0
            return ver >= req && ver.major == req.major;
        }
    }
    false
}

/// Resolve dependencies using minimal version selection.
///
/// For each dependency:
/// 1. If in lockfile: use locked version
/// 2. If local path dependency: no resolution needed (path is the resolution)
/// 3. If registry dependency without lockfile: resolve fails (no registry yet)
pub fn resolve(
    root_deps: &[Requirement],
    lockfile: Option<&crate::lockfile::Lockfile>,
    available: &HashMap<String, Vec<AvailableVersion>>,
) -> Result<Resolution, PkgError> {
    let mut resolved: HashMap<String, ResolvedPackage> = HashMap::new();
    let mut constraints: HashMap<String, Vec<String>> = HashMap::new();

    // Seed with root dependencies
    for req in root_deps {
        constraints
            .entry(req.name.clone())
            .or_default()
            .push(req.version_constraint.clone());
    }

    let mut queue: Vec<String> = root_deps.iter().map(|r| r.name.clone()).collect();

    while let Some(name) = queue.pop() {
        if resolved.contains_key(&name) {
            continue;
        }

        // Check lockfile first
        if let Some(lock) = lockfile {
            if let Some(pkg) = lock.packages.iter().find(|p| p.name == name) {
                resolved.insert(
                    name.clone(),
                    ResolvedPackage {
                        version: pkg.version.clone(),
                        is_macro: pkg.is_macro,
                        deps: pkg.deps.clone(),
                        source: pkg.source.clone(),
                    },
                );
                // Queue transitive dependencies
                for dep in &pkg.deps {
                    if !resolved.contains_key(dep) {
                        constraints.entry(dep.clone()).or_default().push("*".to_string());
                        queue.push(dep.clone());
                    }
                }
                continue;
            }
        }

        let versions = available.get(&name).ok_or_else(|| {
            PkgError::Resolution(format!("package '{name}' not found in available packages"))
        })?;

        let name_constraints = constraints.get(&name).ok_or_else(|| {
            PkgError::Resolution(format!("no constraints for '{name}'"))
        })?;

        let selected = find_min_satisfying(versions, name_constraints)
            .ok_or_else(|| {
                PkgError::Resolution(format!(
                    "no version of '{name}' satisfies constraints {:?}",
                    name_constraints
                ))
            })?;

        for dep in &selected.deps {
            if !resolved.contains_key(&dep.name) {
                constraints
                    .entry(dep.name.clone())
                    .or_default()
                    .push(dep.version_constraint.clone());
                queue.push(dep.name.clone());
            }
        }

        resolved.insert(
            name.clone(),
            ResolvedPackage {
                version: selected.version.clone(),
                is_macro: selected.is_macro,
                deps: selected.deps.iter().map(|d| d.name.clone()).collect(),
                source: selected.source.clone(),
            },
        );
    }

    Ok(Resolution { packages: resolved })
}

/// Find the minimum version in `available` that satisfies all constraints.
fn find_min_satisfying<'a>(
    versions: &'a [AvailableVersion],
    constraints: &[String],
) -> Option<&'a AvailableVersion> {
    let mut satisfying: Vec<&AvailableVersion> = versions
        .iter()
        .filter(|v| constraints.iter().all(|c| satisfies_constraint(&v.version, c)))
        .collect();

    if satisfying.is_empty() {
        return None;
    }

    satisfying.sort_by(|a, b| {
        let va = semver::Version::parse(&a.version).unwrap();
        let vb = semver::Version::parse(&b.version).unwrap();
        va.cmp(&vb)
    });

    Some(satisfying[0])
}
