use dunce::canonicalize;
use std::path::{Path, PathBuf};

/// `validate_path`.
pub fn validate_path(worktree_root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative_path);
    if relative.is_absolute() {
        return Err(format!(
            "path '{relative_path}' is absolute; must be relative to worktree"
        ));
    }

    let canonical_root = if worktree_root.exists() {
        match canonicalize(worktree_root) {
            Ok(c) => c,
            Err(_) => path_clean::PathClean::clean(worktree_root),
        }
    } else {
        path_clean::PathClean::clean(worktree_root)
    };

    let candidate = canonical_root.join(relative);
    let normalized = path_clean::PathClean::clean(&candidate);

    if normalized == canonical_root {
        return Err(format!(
            "path '{relative_path}' resolves to worktree root, not a file"
        ));
    }
    if !normalized.starts_with(&canonical_root) {
        if worktree_root.exists() {
            if let (Ok(can_child), Ok(can_parent)) =
                (canonicalize(&normalized), canonicalize(&canonical_root))
            {
                if can_child.starts_with(can_parent) {
                    return Ok(normalized);
                }
            }
        }
        return Err(format!(
            "path '{}' escapes worktree '{}'",
            relative_path,
            canonical_root.display()
        ));
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_path_traversal_attack() {
        let dir = TempDir::new().unwrap();
        let result = validate_path(dir.path(), "../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_absolute_path_rejected() {
        let dir = TempDir::new().unwrap();
        let result = validate_path(dir.path(), "/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_valid_path() {
        let dir = TempDir::new().unwrap();
        let result = validate_path(dir.path(), "src/main.rs");
        assert!(result.is_ok());
    }
}
