use crate::manifest::load_manifest;
use std::path::{Path, PathBuf};

pub struct Workspace {
    pub root: PathBuf,
    pub members: Vec<PathBuf>,
    pub manifest: crate::manifest::PackageManifest,
}

/// Detect if `start` is within a workspace by walking up looking for
/// a glyim.toml with a [workspace] section.
pub fn detect_workspace(start: &Path) -> Option<Workspace> {
    let mut current = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let manifest_path = current.join("glyim.toml");
        if manifest_path.exists()
            && let Ok(manifest) = load_manifest(&manifest_path)
            && let Some(ws) = &manifest.workspace
        {
            let members = resolve_member_globs(&current, &ws.members)?;
            return Some(Workspace {
                root: current,
                members,
                manifest,
            });
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Resolve glob patterns like "crates/*" to actual directories containing glyim.toml.
pub fn resolve_member_globs(root: &Path, patterns: &[String]) -> Option<Vec<PathBuf>> {
    let mut members = Vec::new();
    for pattern in patterns {
        if let Some(dir) = pattern.strip_suffix("/*") {
            let full_dir = root.join(dir);
            if let Ok(entries) = std::fs::read_dir(&full_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && path.join("glyim.toml").exists() {
                        members.push(path);
                    }
                }
            }
        } else {
            let full_path = root.join(pattern);
            if full_path.join("glyim.toml").exists() {
                members.push(full_path);
            }
        }
    }
    Some(members)
}
