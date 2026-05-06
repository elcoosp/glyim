use glyim_pkg::manifest::PackageManifest;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::toposort;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum GraphError {
    Io(std::io::Error),
    Manifest(glyim_pkg::PkgError),
    Cycle,
    WorkspaceNotFound,
    MemberNotFound(String),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Manifest(e) => write!(f, "manifest error: {e}"),
            Self::Cycle => write!(f, "circular dependency detected"),
            Self::WorkspaceNotFound => write!(f, "no workspace or package found"),
            Self::MemberNotFound(name) => write!(f, "workspace member '{}' not found", name),
        }
    }
}

impl std::error::Error for GraphError {}

/// A node in the package dependency graph.
#[derive(Debug, Clone)]
pub struct PackageNode {
    pub name: String,
    pub dir: PathBuf,
    pub manifest: PackageManifest,
}

/// An edge in the package dependency graph.
#[derive(Debug, Clone)]
pub struct DependencyEdge {
    pub dep_name: String,
    pub is_macro: bool,
}

/// The directed acyclic graph of packages and their dependencies.
pub struct PackageGraph {
    graph: DiGraph<PackageNode, DependencyEdge>,
    name_to_idx: HashMap<String, NodeIndex>,
}

impl PackageGraph {
    /// Discover the package graph from a root directory.
    /// If the root contains a `glyim.toml` with a `[workspace]` section, all members are included.
    /// Otherwise a single‑package graph is returned.
    pub fn discover(root: &Path) -> Result<Self, GraphError> {
        let root = std::fs::canonicalize(root).map_err(GraphError::Io)?;
        let manifest_path = root.join("glyim.toml");
        if !manifest_path.exists() {
            return Err(GraphError::WorkspaceNotFound);
        }

        let root_manifest = glyim_pkg::manifest::load_manifest(&manifest_path)
            .map_err(GraphError::Manifest)?;

        let mut graph = DiGraph::new();
        let mut name_to_idx = HashMap::new();

        // If the root has a workspace, discover all members
        if let Some(ws) = &root_manifest.workspace {
            let members =
                glyim_pkg::workspace::resolve_member_globs(&root, &ws.members)
                    .ok_or_else(|| GraphError::MemberNotFound(ws.members.join(", ")))?;

            for member_dir in members {
                Self::add_package_from_dir(&member_dir, &mut graph, &mut name_to_idx)?;
            }
        } else {
            // Single package
            Self::add_package_from_dir(&root, &mut graph, &mut name_to_idx)?;
        }

        // Add dependency edges
        let indices: Vec<NodeIndex> = name_to_idx.values().copied().collect();
        for idx in indices {
            let node = graph[idx].clone();
            // Dependencies
            for dep_name in node.manifest.dependencies.keys() {
                if let Some(&dep_idx) = name_to_idx.get(dep_name) {
                    graph.add_edge(dep_idx, idx, DependencyEdge {
                        dep_name: dep_name.clone(),
                        is_macro: false,
                    });
                } else {
                    // external dependency – ignore (handled by lockfile later)
                    tracing::debug!("external dependency '{}' not a workspace member", dep_name);
                }
            }
            // Macro dependencies
            for dep_name in node.manifest.macros.keys() {
                if let Some(&dep_idx) = name_to_idx.get(dep_name) {
                    graph.add_edge(dep_idx, idx, DependencyEdge {
                        dep_name: dep_name.clone(),
                        is_macro: true,
                    });
                }
            }
        }

        Ok(Self { graph, name_to_idx })
    }

    /// Add a package from a directory containing `glyim.toml`.
    fn add_package_from_dir(
        dir: &Path,
        graph: &mut DiGraph<PackageNode, DependencyEdge>,
        name_to_idx: &mut HashMap<String, NodeIndex>,
    ) -> Result<(), GraphError> {
        let manifest_path = dir.join("glyim.toml");
        let manifest = glyim_pkg::manifest::load_manifest(&manifest_path)
            .map_err(GraphError::Manifest)?;
        let name = manifest.package.name.clone();
        let node = PackageNode {
            name: name.clone(),
            dir: dir.to_path_buf(),
            manifest,
        };
        let idx = graph.add_node(node);
        name_to_idx.insert(name, idx);
        Ok(())
    }

    /// Return packages in topological (build) order.
    pub fn build_order(&self) -> Result<Vec<&PackageNode>, GraphError> {
        match toposort(&self.graph, None) {
            Ok(order) => Ok(order.iter().map(|idx| &self.graph[*idx]).collect()),
            Err(cycle) => {
                let node = self.graph.node_weight(cycle.node_id());
                let _node_name = node.map(|n| n.name.clone()).unwrap_or_else(|| "unknown".to_string());
                Err(GraphError::Cycle)
            }
        }
    }

    /// Return all packages that directly depend on the given package.
    pub fn direct_dependents(&self, package_name: &str) -> Vec<&PackageNode> {
        if let Some(&idx) = self.name_to_idx.get(package_name) {
            self.graph.neighbors_directed(idx, petgraph::Direction::Outgoing)
                .map(|n| &self.graph[n])
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Return all transitive dependents (packages affected by a change).
    pub fn transitive_dependents(&self, package_name: &str) -> Vec<&PackageNode> {
        if let Some(&start) = self.name_to_idx.get(package_name) {
            let mut visited = std::collections::HashSet::new();
            let mut stack = vec![start];
            let mut result = Vec::new();
            while let Some(node) = stack.pop() {
                if !visited.insert(node) {
                    continue;
                }
                result.push(node);
                for neighbor in self.graph.neighbors_directed(node, petgraph::Direction::Outgoing) {
                    stack.push(neighbor);
                }
            }
            result.into_iter().filter(|&n| n != start).map(|n| &self.graph[n]).collect()
        } else {
            Vec::new()
        }
    }

    /// Number of packages in the graph.
    pub fn len(&self) -> usize {
        self.graph.node_count()
    }

    /// Get a package by name.
    pub fn get(&self, name: &str) -> Option<&PackageNode> {
        self.name_to_idx.get(name).map(|&idx| &self.graph[idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_manifest(dir: &Path, name: &str, deps: &[&str]) {
        let toml_content = format!(
            r#"[package]
name = "{}"
version = "0.1.0"
"#, name);
        let mut deps_str = String::new();
        for dep in deps {
            deps_str.push_str(&format!("{} = {{ version = \"*\" }}\n", dep));
        }
        let full = if deps.is_empty() {
            toml_content
        } else {
            format!("{}\n[dependencies]\n{}", toml_content, deps_str)
        };
        fs::write(dir.join("glyim.toml"), full).unwrap();
    }

    #[test]
    fn single_package_graph() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(dir.path(), "my_app", &[]);
        let graph = PackageGraph::discover(dir.path()).unwrap();
        assert_eq!(graph.len(), 1);
        assert!(graph.get("my_app").is_some());
    }

    #[test]
    fn workspace_with_two_members() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("glyim.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n").unwrap();
        let a_dir = dir.path().join("crates/a");
        fs::create_dir_all(&a_dir).unwrap();
        write_manifest(&a_dir, "a", &["b"]);
        let b_dir = dir.path().join("crates/b");
        fs::create_dir_all(&b_dir).unwrap();
        write_manifest(&b_dir, "b", &[]);

        let graph = PackageGraph::discover(dir.path()).unwrap();
        assert_eq!(graph.len(), 2);
        let order = graph.build_order().unwrap();
        // b must come before a
        assert_eq!(order[0].name, "b");
        assert_eq!(order[1].name, "a");
    }

    #[test]
    fn workspace_with_circular_dependency() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("glyim.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\n").unwrap();
        let a_dir = dir.path().join("a");
        fs::create_dir_all(&a_dir).unwrap();
        write_manifest(&a_dir, "a", &["b"]);
        let b_dir = dir.path().join("b");
        fs::create_dir_all(&b_dir).unwrap();
        write_manifest(&b_dir, "b", &["a"]);

        let graph = PackageGraph::discover(dir.path()).unwrap();
        assert!(graph.build_order().is_err());
    }
}
