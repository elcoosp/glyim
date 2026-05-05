use crate::types::TestDef;
use crate::config::PriorityMode;

pub fn sort_tests(tests: &mut [TestDef], mode: PriorityMode) {
    match mode {
        PriorityMode::DeclarationOrder => {} // already in order
        PriorityMode::FastFirst => {
            tests.sort_by_key(|t| t.name.clone()); // placeholder: could use historical duration
        }
        PriorityMode::RecentFailuresFirst => {
            // Placeholder: push tests that failed recently to front
            // In a real impl we would look up history DB
        }
    }
}
