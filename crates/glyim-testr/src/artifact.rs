use crate::types::TestDef;
use std::path::PathBuf;

pub struct CompiledArtifact {
    pub bin_path: PathBuf,
    pub test_defs: Vec<TestDef>,
    pub(crate) _temp_dir: tempfile::TempDir,
}
