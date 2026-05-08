use crate::types::TestDef;
use std::path::PathBuf;

pub struct CompiledArtifact {
    pub test_defs: Vec<TestDef>,
    /// Single binary path (when only one test compiled)
    pub bin_path: Option<PathBuf>,
    /// Multiple binaries (test_name -> binary_path)
    pub per_test_binaries: Vec<(String, PathBuf)>,
    pub(crate) _temp_dir: tempfile::TempDir,
}
