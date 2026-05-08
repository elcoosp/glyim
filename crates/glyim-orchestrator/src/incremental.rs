use glyim_macro_vfs::ContentHash;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CrossPackageIncremental {
    package_roots: HashMap<String, ContentHash>,
    dep_fingerprints: HashMap<String, HashMap<String, ContentHash>>,
}

impl CrossPackageIncremental {
    pub fn new() -> Self { Self::default() }
    pub fn load(workspace_root: &std::path::Path) -> Result<Self, String> {
        let state_dir = workspace_root.join(".glyim/incremental");
        let state_file = state_dir.join("cross-package.bin");
        if !state_file.exists() { return Ok(Self::new()); }
        let data = std::fs::read(&state_file).map_err(|e| format!("read: {e}"))?;
        postcard::from_bytes(&data).map_err(|e| format!("deser: {e}"))
    }
    pub fn save(&self, workspace_root: &std::path::Path) -> Result<(), String> {
        let state_dir = workspace_root.join(".glyim/incremental");
        std::fs::create_dir_all(&state_dir).map_err(|e| format!("mkdir: {e}"))?;
        let state_file = state_dir.join("cross-package.bin");
        let data = postcard::to_allocvec(self).map_err(|e| format!("ser: {e}"))?;
        std::fs::write(&state_file, data).map_err(|e| format!("write: {e}"))
    }
    pub fn compute_affected_packages(&self, changed_packages: &[String], graph: &super::graph::PackageGraph) -> Vec<String> {
        let mut affected = Vec::new();
        let mut visited = std::collections::HashSet::new();
        for pkg_name in changed_packages {
            let mut stack = vec![pkg_name.clone()];
            while let Some(current) = stack.pop() {
                if !visited.insert(current.clone()) { continue; }
                affected.push(current.clone());
                for dep_node in graph.direct_dependents(&current) {
                    stack.push(dep_node.name.clone());
                }
            }
        }
        affected
    }
    pub fn update_package_root(&mut self, package: &str, root: ContentHash) {
        self.package_roots.insert(package.to_string(), root);
    }
    pub fn get_package_root(&self, package: &str) -> Option<ContentHash> {
        self.package_roots.get(package).copied()
    }
    pub fn record_dep_fingerprint(&mut self, package: &str, dep_name: &str, dep_root: ContentHash) {
        self.dep_fingerprints.entry(package.to_string()).or_default().insert(dep_name.to_string(), dep_root);
    }
    pub fn did_dependency_change(&self, package: &str, dep_name: &str, current_dep_root: ContentHash) -> bool {
        self.dep_fingerprints.get(package).and_then(|deps| deps.get(dep_name)).map(|old| *old != current_dep_root).unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn write_manifest(dir: &std::path::Path, name: &str, deps: &[&str]) {
        let mut deps_str = String::new();
        for dep in deps { deps_str.push_str(&format!("{} = {{ version = \"*\" }}\n", dep)); }
        let full = if deps.is_empty() {
            format!("[package]\nname = \"{}\"\nversion = \"0.1.0\"\n", name)
        } else {
            format!("[package]\nname = \"{}\"\nversion = \"0.1.0\"\n[dependencies]\n{}", name, deps_str)
        };
        fs::write(dir.join("glyim.toml"), full).unwrap();
    }
    fn create_test_graph(dir: &tempfile::TempDir) -> super::super::graph::PackageGraph {
        let a_dir = dir.path().join("a");
        let b_dir = dir.path().join("b");
        fs::create_dir_all(&a_dir).unwrap();
        fs::create_dir_all(&b_dir).unwrap();
        write_manifest(&a_dir, "a", &["b"]);
        write_manifest(&b_dir, "b", &[]);
        let ws_toml = "[workspace]\nmembers = [\"a\", \"b\"]\n";
        fs::write(dir.path().join("glyim.toml"), ws_toml).unwrap();
        super::super::graph::PackageGraph::discover(dir.path()).unwrap()
    }
    #[test]
    fn load_save_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = CrossPackageIncremental::new();
        let root = ContentHash::of(b"pkg_a_root");
        state.update_package_root("a", root);
        state.record_dep_fingerprint("a", "b", ContentHash::of(b"dep_b"));
        state.save(dir.path()).unwrap();
        let loaded = CrossPackageIncremental::load(dir.path()).unwrap();
        assert_eq!(loaded.get_package_root("a"), Some(root));
        assert!(loaded.did_dependency_change("a", "b", ContentHash::of(b"different")));
    }
    #[test]
    fn compute_affected_packages_single_change() {
        let dir = tempfile::tempdir().unwrap();
        let graph = create_test_graph(&dir);
        let state = CrossPackageIncremental::new();
        let affected = state.compute_affected_packages(&["b".to_string()], &graph);
        assert!(affected.contains(&"b".to_string()));
        assert!(affected.contains(&"a".to_string()));
    }
    #[test]
    fn compute_affected_packages_no_dependents() {
        let dir = tempfile::tempdir().unwrap();
        let graph = create_test_graph(&dir);
        let state = CrossPackageIncremental::new();
        let affected = state.compute_affected_packages(&["a".to_string()], &graph);
        assert_eq!(affected, vec!["a".to_string()]);
    }
    #[test]
    fn did_dependency_change_new() {
        let state = CrossPackageIncremental::new();
        assert!(state.did_dependency_change("a", "b", ContentHash::of(b"any")));
    }
    #[test]
    fn did_dependency_change_unchanged() {
        let mut state = CrossPackageIncremental::new();
        let dep_root = ContentHash::of(b"dep_v1");
        state.record_dep_fingerprint("a", "b", dep_root);
        assert!(!state.did_dependency_change("a", "b", dep_root));
    }
    #[test]
    fn did_dependency_change_changed() {
        let mut state = CrossPackageIncremental::new();
        state.record_dep_fingerprint("a", "b", ContentHash::of(b"old"));
        assert!(state.did_dependency_change("a", "b", ContentHash::of(b"new")));
    }
}
