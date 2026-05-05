use similar::TextDiff;
use std::path::PathBuf;
use std::fs;

pub struct SnapshotStore {
    snapshot_dir: PathBuf,
}

impl SnapshotStore {
    pub fn new(snapshot_dir: PathBuf) -> Self {
        Self { snapshot_dir }
    }

    pub fn assert_snapshot(&self, test_name: &str, actual: &str) -> Result<(), String> {
        let snap_path = self.snapshot_dir.join(format!("{}.snap", test_name));
        if snap_path.exists() {
            let expected = fs::read_to_string(&snap_path)
                .map_err(|e| format!("read snapshot: {e}"))?;
            if expected != actual {
                let diff = TextDiff::from_lines(expected.as_str(), actual);
                Err(format!("snapshot mismatch for {}:\n{}", test_name, diff.unified_diff()))
            } else {
                Ok(())
            }
        } else {
            fs::write(&snap_path, actual)
                .map_err(|e| format!("write snapshot: {e}"))?;
            Ok(())
        }
    }
}
