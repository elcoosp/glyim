//! Worktree path validation for file appliers.
//!
//! Every path an applier touches is resolved against the session's
//! worktree root and rejected when it escapes. Rejections are typed
//! ([`PathValidationError`]) rather than strings so call sites can match
//! on the rejection class and tests can assert on constructors instead
//! of message substrings.

use std::fmt;
use std::path::{Path, PathBuf};

use dunce::canonicalize;

/// Why [`validate_path`] rejected a relative path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathValidationError {
    /// The path was absolute; appliers only accept worktree-relative
    /// paths.
    Absolute {
        /// The rejected path as given.
        given: String,
    },
    /// The path normalized to the worktree root itself, not a file
    /// inside it.
    ResolvesToRoot {
        /// The rejected path as given.
        given: String,
    },
    /// The path normalized outside the worktree (the `..`-traversal
    /// class this module exists to close).
    EscapesWorktree {
        /// The rejected path as given.
        given: String,
        /// The worktree root it escaped.
        root: PathBuf,
    },
}

impl fmt::Display for PathValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absolute { given } => {
                write!(
                    f,
                    "path '{given}' is absolute; must be relative to worktree"
                )
            }
            Self::ResolvesToRoot { given } => {
                write!(f, "path '{given}' resolves to worktree root, not a file")
            }
            Self::EscapesWorktree { given, root } => {
                write!(f, "path '{given}' escapes worktree '{}'", root.display())
            }
        }
    }
}

impl std::error::Error for PathValidationError {}

/// Resolve `relative_path` against `worktree_root`, rejecting absolute
/// paths, root resolutions, and traversal outside the worktree.
///
/// # Errors
///
/// Returns a typed [`PathValidationError`] naming the rejection class;
/// the `Display` impl carries the same message the former
/// `Result<_, String>` produced.
pub fn validate_path(
    worktree_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, PathValidationError> {
    let relative = Path::new(relative_path);
    if relative.is_absolute() {
        return Err(PathValidationError::Absolute {
            given: relative_path.to_string(),
        });
    }

    let canonical_root = if worktree_root.exists() {
        canonicalize(worktree_root).unwrap_or_else(|_| path_clean::PathClean::clean(worktree_root))
    } else {
        path_clean::PathClean::clean(worktree_root)
    };

    let candidate = canonical_root.join(relative);
    let normalized = path_clean::PathClean::clean(&candidate);

    if normalized == canonical_root {
        return Err(PathValidationError::ResolvesToRoot {
            given: relative_path.to_string(),
        });
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
        return Err(PathValidationError::EscapesWorktree {
            given: relative_path.to_string(),
            root: canonical_root,
        });
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_path_traversal_attack() {
        let dir = TempDir::new().expect("tmp");
        let result = validate_path(dir.path(), "../../etc/passwd");
        assert!(result.is_err());
        assert!(
            matches!(result, Err(PathValidationError::EscapesWorktree { .. })),
            "{result:?}"
        );
    }

    #[test]
    fn test_absolute_path_rejected() {
        let dir = TempDir::new().expect("tmp");
        let result = validate_path(dir.path(), "/etc/passwd");
        assert!(matches!(result, Err(PathValidationError::Absolute { .. })));
    }

    #[test]
    fn test_root_resolution_rejected() {
        let dir = TempDir::new().expect("tmp");
        let result = validate_path(dir.path(), ".");
        assert!(matches!(
            result,
            Err(PathValidationError::ResolvesToRoot { .. })
        ));
    }

    #[test]
    fn test_valid_path() {
        let dir = TempDir::new().expect("tmp");
        let result = validate_path(dir.path(), "src/main.rs");
        assert!(result.is_ok());
    }
}
