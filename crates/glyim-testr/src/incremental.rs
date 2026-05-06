use crate::types::TestDef;
use std::collections::HashSet;

pub struct DependencyGraph;

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self
    }
    pub fn affected_tests(
        &self,
        _changed_files: &HashSet<&str>,
        all_tests: &[TestDef],
    ) -> Vec<TestDef> {
        // Placeholder: return all tests
        all_tests.to_vec()
    }
}
