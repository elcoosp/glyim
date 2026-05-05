#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PriorityMode {
    DeclarationOrder,
    RecentFailuresFirst,
    FastFirst,
}

#[derive(Debug, Clone)]
pub struct TestConfig {
    pub filter: Option<String>,
    pub include_ignored: bool,
    pub nocapture: bool,
    pub watch: bool,
    pub priority_mode: PriorityMode,
    pub history_db_path: String,
    pub debounce_ms: u64,
    pub num_jobs: usize,
    pub timeout_secs: u64,
    pub optimize_check: bool,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            filter: None,
            include_ignored: false,
            nocapture: false,
            watch: false,
            priority_mode: PriorityMode::RecentFailuresFirst,
            history_db_path: "target/glyim/test-history.db".into(),
            debounce_ms: 100,
            num_jobs: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
            timeout_secs: 30,
            optimize_check: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_positive_timeout() {
        let c = TestConfig::default();
        assert!(c.timeout_secs > 0);
    }

    #[test]
    fn default_config_has_at_least_one_job() {
        let c = TestConfig::default();
        assert!(c.num_jobs >= 1);
    }
}
