use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Tracks dependencies between files. When a file changes, all files that depend on it
/// must be re-analyzed.
pub struct DependencyGraph {
    // Forward: file -> set of files it depends on (imports)
    deps: HashMap<PathBuf, HashSet<PathBuf>>,
    // Reverse: file -> set of files that depend on it (reverse deps)
    rev_deps: HashMap<PathBuf, HashSet<PathBuf>>,
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyGraph {
    /// new.
    #[must_use]
    pub fn new() -> Self {
        Self {
            deps: HashMap::new(),
            rev_deps: HashMap::new(),
        }
    }

    /// Add a dependency: `file` depends on `dep`
    pub fn add_dep(&mut self, file: PathBuf, dep: PathBuf) {
        self.deps
            .entry(file.clone())
            .or_default()
            .insert(dep.clone());
        self.rev_deps.entry(dep).or_default().insert(file);
    }

    /// Remove all dependencies for a file (when it's closed or re-analyzed)
    pub fn clear_deps(&mut self, file: &PathBuf) {
        if let Some(deps) = self.deps.remove(file) {
            for dep in deps {
                if let Some(rev) = self.rev_deps.get_mut(&dep) {
                    rev.remove(file);
                }
            }
        }
        // Also remove from rev_deps as a dependent
        if let Some(dependents) = self.rev_deps.remove(file) {
            for dep_file in dependents {
                if let Some(fwd) = self.deps.get_mut(&dep_file) {
                    fwd.remove(file);
                }
            }
        }
    }

    /// Get all files that need to be re-analyzed when `file` changes.
    #[must_use]
    pub fn affected_files(&self, file: &Path) -> HashSet<PathBuf> {
        let mut affected = HashSet::new();
        let mut stack = vec![file.to_path_buf()];
        while let Some(f) = stack.pop() {
            if affected.insert(f.clone())
                && let Some(dependents) = self.rev_deps.get(&f)
            {
                for dep in dependents {
                    stack.push(dep.clone());
                }
            }
        }
        affected
    }

    /// Get direct dependencies of a file (for debugging)
    #[must_use]
    pub fn dependencies(&self, file: &PathBuf) -> Option<&HashSet<PathBuf>> {
        self.deps.get(file)
    }

    /// Re-point every edge that names `old` — as the file or as a
    /// dependency — to `new` (LSP-FILE-RENAME).
    ///
    /// The rename of an editor-owned document must not lose the graph's
    /// edges: a file's dependents were invalidated by its *content*, and
    /// the content did not move — only the path it is keyed by did. The
    /// whole node moves in one step (forward edges, reverse edges, both
    /// endpoint maps), so `affected_files` and the driver's dependent
    /// refresh keep working through the new path. A `new` path that
    /// already carries edges merges them — the sets union, never drop.
    /// A no-op when the old path carries no edges, or when old and new
    /// are the same path.
    pub fn rename_node(&mut self, old: &Path, new: &Path) {
        if old == new {
            return;
        }
        let old_deps = self.deps.remove(old);
        let old_dependents = self.rev_deps.remove(old);
        if old_deps.is_none() && old_dependents.is_none() {
            return;
        }
        if let Some(deps) = &old_deps {
            for dep in deps {
                if dep == old {
                    // Self-edges do not occur (the driver never records
                    // a file depending on itself); a defensive skip
                    // here keeps a malformed graph from panicking on
                    // the re-point.
                    continue;
                }
                if let Some(rev) = self.rev_deps.get_mut(dep) {
                    rev.remove(old);
                    rev.insert(new.to_path_buf());
                }
            }
        }
        if let Some(dependents) = &old_dependents {
            for dependent in dependents {
                if dependent == old {
                    continue;
                }
                if let Some(fwd) = self.deps.get_mut(dependent) {
                    fwd.remove(old);
                    fwd.insert(new.to_path_buf());
                }
            }
        }
        if let Some(deps) = old_deps {
            self.deps.entry(new.to_path_buf()).or_default().extend(deps);
        }
        if let Some(dependents) = old_dependents {
            self.rev_deps
                .entry(new.to_path_buf())
                .or_default()
                .extend(dependents);
        }
    }
}
