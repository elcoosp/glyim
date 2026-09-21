/// security.
pub mod security;

use std::fs;
use std::path::Path;

use crate::domain_types::ApplyLimits;
use crate::error::{ApplyError, PilotError};
use crate::protocol::types::FileOp;
use security::validate_path;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// ApplyResult.
pub struct ApplyResult {
    /// Struct.
    pub path: String,
    /// Struct.
    pub action: ApplyAction,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// ApplyAction.
pub enum ApplyAction {
    /// Variant.
    Created,
    /// Variant.
    Modified,
    /// Variant.
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// PlannedChange.
pub struct PlannedChange {
    /// Struct.
    pub path: String,
    /// Struct.
    pub action: PlannedAction,
    /// Struct.
    pub current_content_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// PlannedAction.
pub enum PlannedAction {
    /// Variant.
    Create,
    /// Variant.
    Overwrite,
    /// Variant.
    Modify,
    /// Variant.
    Delete,
}

struct Backup {
    rel_path: String,
    original_content: Option<String>,
}

/// apply_ops.
pub fn apply_ops(
    worktree_root: &Path,
    ops: &[FileOp],
    limits: &ApplyLimits,
) -> Result<Vec<ApplyResult>, PilotError> {
    validate_limits(ops, limits)?;
    let backups = create_backups(worktree_root, ops)?;
    let mut results = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        match apply_op_atomic(worktree_root, op) {
            Ok(result) => {
                tracing::debug!(path = %result.path, action = ?result.action, "applied {}/{}", i + 1, ops.len());
                results.push(result);
            }
            Err(e) => {
                tracing::error!(op_index = i, error = %e, "apply failed, rolling back");
                rollback(worktree_root, &backups, &results);
                return Err(PilotError::Apply(ApplyError::RolledBack {
                    detail: format!(
                        "operation {} of {} failed: {} (rollback succeeded)",
                        i + 1,
                        ops.len(),
                        e
                    ),
                }));
            }
        }
    }
    Ok(results)
}

/// preview_ops.
pub fn preview_ops(worktree_root: &Path, ops: &[FileOp]) -> Result<Vec<PlannedChange>, PilotError> {
    ops.iter().map(|op| preview_op(worktree_root, op)).collect()
}

/// apply_ops_async.
pub async fn apply_ops_async(
    worktree_root: std::path::PathBuf,
    ops: Vec<FileOp>,
    limits: ApplyLimits,
) -> Result<Vec<ApplyResult>, PilotError> {
    tokio::task::spawn_blocking(move || apply_ops(&worktree_root, &ops, &limits))
        .await
        .map_err(|je| {
            PilotError::Apply(ApplyError::TaskJoin {
                operation: "apply_ops".into(),
                reason: if je.is_panic() {
                    "task panicked".into()
                } else {
                    "task cancelled".into()
                },
            })
        })?
}

/// preview_ops_async.
pub async fn preview_ops_async(
    worktree_root: std::path::PathBuf,
    ops: Vec<FileOp>,
) -> Result<Vec<PlannedChange>, PilotError> {
    tokio::task::spawn_blocking(move || preview_ops(&worktree_root, &ops))
        .await
        .map_err(|je| {
            PilotError::Apply(ApplyError::TaskJoin {
                operation: "preview_ops".into(),
                reason: if je.is_panic() {
                    "task panicked".into()
                } else {
                    "task cancelled".into()
                },
            })
        })?
}

fn validate_limits(ops: &[FileOp], limits: &ApplyLimits) -> Result<(), PilotError> {
    if ops.len() > limits.max_ops_per_block {
        return Err(PilotError::Limits(format!(
            "ops block contains {} operations (max {})",
            ops.len(),
            limits.max_ops_per_block
        )));
    }
    let mut total: usize = 0;
    for op in ops {
        let len = match op {
            FileOp::Write { content, .. } => content.len(),
            FileOp::Replace { find, replace, .. } => find.len() + replace.len(),
            FileOp::Delete { .. } => 0,
        };
        if len > limits.max_file_size {
            return Err(PilotError::Limits(format!(
                "content for '{}' is {} bytes (max {})",
                op.path(),
                len,
                limits.max_file_size
            )));
        }
        total += len;
    }
    if total > limits.max_total_content {
        return Err(PilotError::Limits(format!(
            "total content is {} bytes (max {})",
            total, limits.max_total_content
        )));
    }
    Ok(())
}

fn create_backups(worktree_root: &Path, ops: &[FileOp]) -> Result<Vec<Backup>, PilotError> {
    let mut backups = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for op in ops {
        let rel_path = op.path();
        if seen.contains(rel_path) {
            continue;
        }
        seen.insert(rel_path.to_string());
        let abs_path =
            validate_path(worktree_root, rel_path).map_err(|reason| PilotError::PathEscape {
                path: rel_path.to_string(),
                root: worktree_root.display().to_string(),
                reason,
            })?;
        let original_content = if abs_path.exists() {
            Some(fs::read_to_string(&abs_path).map_err(|e| {
                PilotError::Apply(ApplyError::Io {
                    path: rel_path.to_string(),
                    operation: "read_for_backup".into(),
                    source: e,
                })
            })?)
        } else {
            None
        };
        backups.push(Backup {
            rel_path: rel_path.to_string(),
            original_content,
        });
    }
    Ok(backups)
}

fn rollback(worktree_root: &Path, backups: &[Backup], results: &[ApplyResult]) {
    for result in results {
        let backup = backups.iter().find(|b| b.rel_path == result.path);
        match backup {
            Some(b) => match &b.original_content {
                Some(content) => {
                    let abs_path = match validate_path(worktree_root, &b.rel_path) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    if let Err(e) = fs::write(&abs_path, content) {
                        tracing::error!(path = &b.rel_path, error = %e, "rollback: failed to restore");
                    }
                }
                None => {
                    let abs_path = match validate_path(worktree_root, &b.rel_path) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    if abs_path.exists() {
                        let _ = fs::remove_file(&abs_path);
                    }
                }
            },
            None => {
                let abs_path = match validate_path(worktree_root, &result.path) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if abs_path.exists() {
                    let _ = fs::remove_file(&abs_path);
                }
            }
        }
    }
}

fn apply_op_atomic(worktree_root: &Path, op: &FileOp) -> Result<ApplyResult, PilotError> {
    match op {
        FileOp::Write { path, content } => apply_write_atomic(worktree_root, path, content),
        FileOp::Replace {
            path,
            find,
            replace,
        } => apply_replace_atomic(worktree_root, path, find, replace),
        FileOp::Delete { path } => apply_delete(worktree_root, path),
    }
}

fn apply_write_atomic(
    worktree_root: &Path,
    rel_path: &str,
    content: &str,
) -> Result<ApplyResult, PilotError> {
    let abs_path =
        validate_path(worktree_root, rel_path).map_err(|reason| PilotError::PathEscape {
            path: rel_path.to_string(),
            root: worktree_root.display().to_string(),
            reason,
        })?;
    if let Some(parent) = abs_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            PilotError::Apply(ApplyError::Io {
                path: rel_path.to_string(),
                operation: "create_dir_all".into(),
                source: e,
            })
        })?;
    }
    let existed = abs_path.exists();
    let tmp_path = abs_path.with_extension("glyim-tmp");
    fs::write(&tmp_path, content).map_err(|e| {
        PilotError::Apply(ApplyError::Io {
            path: rel_path.to_string(),
            operation: "write_tmp".into(),
            source: e,
        })
    })?;
    fs::rename(&tmp_path, &abs_path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        PilotError::Apply(ApplyError::Io {
            path: rel_path.to_string(),
            operation: "rename".into(),
            source: e,
        })
    })?;
    Ok(ApplyResult {
        path: rel_path.to_string(),
        action: if existed {
            ApplyAction::Modified
        } else {
            ApplyAction::Created
        },
    })
}

fn apply_replace_atomic(
    worktree_root: &Path,
    rel_path: &str,
    find: &str,
    replace: &str,
) -> Result<ApplyResult, PilotError> {
    let abs_path =
        validate_path(worktree_root, rel_path).map_err(|reason| PilotError::PathEscape {
            path: rel_path.to_string(),
            root: worktree_root.display().to_string(),
            reason,
        })?;
    if !abs_path.exists() {
        return Err(PilotError::Apply(ApplyError::FileNotFound(
            rel_path.to_string(),
        )));
    }
    let existing = fs::read_to_string(&abs_path).map_err(|e| {
        PilotError::Apply(ApplyError::Io {
            path: rel_path.to_string(),
            operation: "read".into(),
            source: e,
        })
    })?;
    let count = existing.matches(find).count();
    if count == 0 {
        return Err(PilotError::Apply(ApplyError::FindNotFound {
            path: rel_path.to_string(),
        }));
    }
    if count > 1 {
        return Err(PilotError::Apply(ApplyError::FindAmbiguous {
            path: rel_path.to_string(),
            count,
        }));
    }
    let new_content = existing.replacen(find, replace, 1);
    let tmp_path = abs_path.with_extension("glyim-tmp");
    fs::write(&tmp_path, &new_content).map_err(|e| {
        PilotError::Apply(ApplyError::Io {
            path: rel_path.to_string(),
            operation: "write_tmp".into(),
            source: e,
        })
    })?;
    fs::rename(&tmp_path, &abs_path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        PilotError::Apply(ApplyError::Io {
            path: rel_path.to_string(),
            operation: "rename".into(),
            source: e,
        })
    })?;
    Ok(ApplyResult {
        path: rel_path.to_string(),
        action: ApplyAction::Modified,
    })
}

fn apply_delete(worktree_root: &Path, rel_path: &str) -> Result<ApplyResult, PilotError> {
    let abs_path =
        validate_path(worktree_root, rel_path).map_err(|reason| PilotError::PathEscape {
            path: rel_path.to_string(),
            root: worktree_root.display().to_string(),
            reason,
        })?;
    if !abs_path.exists() {
        return Err(PilotError::Apply(ApplyError::FileNotFound(
            rel_path.to_string(),
        )));
    }
    fs::remove_file(&abs_path).map_err(|e| {
        PilotError::Apply(ApplyError::Io {
            path: rel_path.to_string(),
            operation: "delete".into(),
            source: e,
        })
    })?;
    Ok(ApplyResult {
        path: rel_path.to_string(),
        action: ApplyAction::Deleted,
    })
}

fn preview_op(worktree_root: &Path, op: &FileOp) -> Result<PlannedChange, PilotError> {
    match op {
        FileOp::Write { path, .. } => {
            let abs_path =
                validate_path(worktree_root, path).map_err(|r| PilotError::PathEscape {
                    path: path.clone(),
                    root: worktree_root.display().to_string(),
                    reason: r,
                })?;
            let exists = abs_path.exists();
            Ok(PlannedChange {
                path: path.clone(),
                action: if exists {
                    PlannedAction::Overwrite
                } else {
                    PlannedAction::Create
                },
                current_content_summary: fs::metadata(&abs_path)
                    .ok()
                    .map(|m| format!("existing file ({} bytes)", m.len())),
            })
        }
        FileOp::Replace { path, .. } => {
            let abs_path =
                validate_path(worktree_root, path).map_err(|r| PilotError::PathEscape {
                    path: path.clone(),
                    root: worktree_root.display().to_string(),
                    reason: r,
                })?;
            if !abs_path.exists() {
                return Err(PilotError::Apply(ApplyError::FileNotFound(path.clone())));
            }
            Ok(PlannedChange {
                path: path.clone(),
                action: PlannedAction::Modify,
                current_content_summary: None,
            })
        }
        FileOp::Delete { path } => {
            let abs_path =
                validate_path(worktree_root, path).map_err(|r| PilotError::PathEscape {
                    path: path.clone(),
                    root: worktree_root.display().to_string(),
                    reason: r,
                })?;
            if !abs_path.exists() {
                return Err(PilotError::Apply(ApplyError::FileNotFound(path.clone())));
            }
            Ok(PlannedChange {
                path: path.clone(),
                action: PlannedAction::Delete,
                current_content_summary: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_and_rollback() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "original a").unwrap();
        let ops = vec![
            FileOp::Write {
                path: "src/a.rs".into(),
                content: "modified a".into(),
            },
            FileOp::Replace {
                path: "src/a.rs".into(),
                find: "nonexistent".into(),
                replace: "x".into(),
            },
        ];
        let result = apply_ops(dir.path(), &ops, &ApplyLimits::default());
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
            "original a"
        );
    }

    #[test]
    fn test_apply_limits() {
        let dir = tempfile::tempdir().unwrap();
        let limits = ApplyLimits {
            max_ops_per_block: 1,
            ..ApplyLimits::default()
        };
        let ops = vec![
            FileOp::Write {
                path: "a.rs".into(),
                content: "a".into(),
            },
            FileOp::Write {
                path: "b.rs".into(),
                content: "b".into(),
            },
        ];
        assert!(matches!(
            apply_ops(dir.path(), &ops, &limits).unwrap_err(),
            PilotError::Limits(_)
        ));
    }
}
