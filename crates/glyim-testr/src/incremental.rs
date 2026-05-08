use crate::types::TestDef;
use std::collections::{HashMap, HashSet};
use serde::{Serialize, Deserialize};

/// Maps each test to the set of source files it depends on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestDependencyGraph {
    test_deps: HashMap<String, HashSet<String>>,
    /// Reverse index: file -> set of tests that depend on it
    file_to_tests: HashMap<String, HashSet<String>>,
}

impl TestDependencyGraph {
    pub fn new() -> Self {
        Self {
            test_deps: HashMap::new(),
            file_to_tests: HashMap::new(),
        }
    }

    /// Record that a test depends on a specific source file.
    pub fn add_dependency(&mut self, test_name: &str, file: &str) {
        self.test_deps
            .entry(test_name.to_string())
            .or_default()
            .insert(file.to_string());
        self.file_to_tests
            .entry(file.to_string())
            .or_default()
            .insert(test_name.to_string());
    }

    /// Remove a test from the graph (e.g., when a test is deleted).
    pub fn remove_test(&mut self, test_name: &str) {
        if let Some(files) = self.test_deps.remove(test_name) {
            for file in files {
                if let Some(tests) = self.file_to_tests.get_mut(&file) {
                    tests.remove(test_name);
                    if tests.is_empty() {
                        self.file_to_tests.remove(&file);
                    }
                }
            }
        }
    }

    /// Given a set of changed source files, return the tests that are affected.
    pub fn affected_tests(&self, changed_files: &HashSet<&str>, all_tests: &[TestDef]) -> Vec<TestDef> {
        // Collect all tests that depend on any of the changed files (via file_to_tests)
        let mut affected_set = HashSet::new();
        for file in changed_files {
            if let Some(tests) = self.file_to_tests.get(*file) {
                for test_name in tests {
                    affected_set.insert(test_name.clone());
                }
            }
        }
        // If no dependencies known or no files changed, run all tests (conservative)
        if affected_set.is_empty() && !changed_files.is_empty() {
            // If we have no file_to_tests mapping, assume all tests are affected
            return all_tests.to_vec();
        }
        // Filter the all_tests list to only those affected
        all_tests.iter()
            .filter(|t| affected_set.contains(&t.name))
            .cloned()
            .collect()
    }

    /// Serialize to disk (optional: we can use the incremental state directory)
    pub fn to_bytes(&self) -> Option<Vec<u8>> {
        postcard::to_allocvec(self).ok()
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        postcard::from_bytes(data).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affected_tests_same_file() {
        let mut dg = TestDependencyGraph::new();
        dg.add_dependency("test_a", "src/main.g");
        dg.add_dependency("test_b", "src/lib.g");
        let tests = vec![
            TestDef { name: "test_a".into(), source_file: "src/main.g".into(), ignored: false, should_panic: false, is_optimize_check: false, tags: vec![] },
            TestDef { name: "test_b".into(), source_file: "src/lib.g".into(), ignored: false, should_panic: false, is_optimize_check: false, tags: vec![] },
        ];
        let mut changed = HashSet::new();
        changed.insert("src/main.g");
        let affected = dg.affected_tests(&changed, &tests);
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0].name, "test_a");
    }
}
