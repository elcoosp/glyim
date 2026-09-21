use glyim_db::{CrateConfig, Database};
use std::path::PathBuf;
use std::sync::Arc;

/// TestDbBuilder.
pub struct TestDbBuilder {
    name: Option<String>,
    target_triple: Option<String>,
    opt_level: u8,
    files: Vec<(PathBuf, Arc<str>)>,
}

impl TestDbBuilder {
    /// new.
    pub fn new() -> Self {
        Self {
            name: None,
            target_triple: None,
            opt_level: 0,
            files: Vec::new(),
        }
    }
    /// name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
    /// target_triple.
    pub fn target_triple(mut self, triple: impl Into<String>) -> Self {
        self.target_triple = Some(triple.into());
        self
    }
    /// opt_level.
    pub fn opt_level(mut self, level: u8) -> Self {
        self.opt_level = level;
        self
    }
    /// file.
    pub fn file(mut self, path: impl Into<PathBuf>, content: impl Into<Arc<str>>) -> Self {
        self.files.push((path.into(), content.into()));
        self
    }
    /// build.
    pub fn build(self) -> Database {
        let config = CrateConfig {
            name: self.name.unwrap_or_else(|| "test".to_string()),
            target_triple: self
                .target_triple
                .unwrap_or_else(|| "x86_64-unknown-linux-gnu".to_string()),
            opt_level: self.opt_level,
        };
        let db = Database::new(config);
        for (path, content) in &self.files {
            db.vfs().add_file_content(path, Arc::clone(content));
        }
        db
    }
}

impl Default for TestDbBuilder {
    fn default() -> Self {
        Self::new()
    }
}
