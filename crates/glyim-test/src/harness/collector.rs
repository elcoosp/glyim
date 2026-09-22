use super::config::{ParsedConfig, TestConfig, TestMode};
use crate::error::TestDiscoveryError;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
/// DiscoveredTest.
pub struct DiscoveredTest {
    /// Struct.
    pub path: PathBuf,
    /// Struct.
    pub name: String,
    /// Struct.
    pub config: TestConfig,
    /// Struct.
    pub source: Arc<str>,
    /// Struct.
    pub revisions: Vec<String>,
}

/// TestCollector.
pub struct TestCollector<'a> {
    root: &'a Path,
}

impl<'a> TestCollector<'a> {
    /// new.
    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }

    /// collect.
    pub fn collect(
        &self,
        filter: Option<&str>,
        mode_override: Option<TestMode>,
    ) -> Result<Vec<Arc<DiscoveredTest>>, TestDiscoveryError> {
        if !self.root.exists() {
            return Err(TestDiscoveryError::RootNotFound(self.root.to_path_buf()));
        }

        let mut tests = Vec::new();

        for entry in walkdir::WalkDir::new(self.root)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("g") {
                continue;
            }
            if let Some(f) = filter
                && !path.to_string_lossy().contains(f)
            {
                continue;
            }

            let source: Arc<str> = std::fs::read_to_string(path)
                .map_err(|e| TestDiscoveryError::ReadFailed {
                    path: path.to_path_buf(),
                    source: e,
                })?
                .into();

            let ParsedConfig {
                config: header_config,
                has_explicit_mode,
            } = super::config::parse_test_config(&source).map_err(|msg| {
                TestDiscoveryError::InvalidConfig {
                    path: path.to_path_buf(),
                    message: msg,
                }
            })?;

            let mut config = TestConfig::default();

            if has_explicit_mode {
                config.mode = header_config.mode;
            } else {
                let dir_mode = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .and_then(|s| s.parse::<TestMode>().ok());
                if let Some(dm) = dir_mode {
                    config.mode = dm;
                }
            }
            if let Some(mode) = mode_override {
                config.mode = mode;
            }

            config.revisions = header_config.revisions;
            config.compile_flags = header_config.compile_flags;
            config.revision_compile_flags = header_config.revision_compile_flags;
            config.error_patterns = header_config.error_patterns;
            config.needs_llvm = header_config.needs_llvm;
            config.ignore = header_config.ignore;
            config.timeout_secs = header_config.timeout_secs;
            config.min_version = header_config.min_version;
            config.only_target = header_config.only_target;
            // Run-check fields. These were previously dropped: `config` was
            // created from the directory-mode default (or `TestConfig::default`)
            // and only a hand-listed subset of the header was copied over, so
            // `// exit-code:` / `// check-stdout:` / `// check-stderr:` /
            // `// aux-file:` had no effect — every run-pass fixture was checked
            // against exit code 0 regardless of what the directive said.
            config.check_stdout = header_config.check_stdout;
            config.check_stderr = header_config.check_stderr;
            config.expected_exit_code = header_config.expected_exit_code;
            config.aux_files = header_config.aux_files;

            let name = path
                .strip_prefix(self.root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");

            let revisions = if config.revisions.is_empty() {
                vec!["default".to_string()]
            } else {
                config.revisions.clone()
            };

            tests.push(Arc::new(DiscoveredTest {
                path: path.to_path_buf(),
                name,
                config,
                source,
                revisions,
            }));
        }

        tests.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(tests)
    }
}
