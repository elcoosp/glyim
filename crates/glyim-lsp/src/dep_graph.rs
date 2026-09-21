use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

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
    /// Returns a set containing the changed file and all its reverse dependencies (transitively).
    #[allow(clippy::ptr_arg)] // &PathBuf matches the stored key type
    pub fn affected_files(&self, file: &PathBuf) -> HashSet<PathBuf> {
        let mut affected = HashSet::new();
        let mut stack = vec![file.clone()];
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
    pub fn dependencies(&self, file: &PathBuf) -> Option<&HashSet<PathBuf>> {
        self.deps.get(file)
    }
}
