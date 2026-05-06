use glyim_compiler::pipeline::BuildMode;
use glyim_merkle::MerkleStore;
use glyim_macro_vfs::{ContentHash, ContentStore, LocalContentStore, RemoteContentStore, RemoteStoreConfig, ActionResult};
use crate::graph::{PackageGraph, PackageNode};
use crate::artifacts::{ArtifactManager, PackageArtifact};
use crate::interface::DependencyInterface;
use crate::incremental::CrossPackageIncremental;
use crate::linker::{LinkConfig, link_multi_object};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct OrchestratorConfig {
    pub mode: BuildMode,
    pub target: Option<String>,
    pub remote_cache_url: Option<String>,
    pub remote_cache_token: Option<String>,
    pub force_rebuild: bool,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            mode: BuildMode::Debug,
            target: None,
            remote_cache_url: None,
            remote_cache_token: None,
            force_rebuild: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OrchestratorReport {
    pub packages_compiled: Vec<String>,
    pub packages_cached: Vec<String>,
    pub packages_failed: Vec<String>,
    pub total_elapsed: std::time::Duration,
    pub per_package_timing: Vec<(String, std::time::Duration)>,
    pub artifacts_pushed: usize,
    pub artifacts_pulled: usize,
}

pub struct PackageGraphOrchestrator {
    workspace_root: PathBuf,
    graph: PackageGraph,
    config: OrchestratorConfig,
    cross_state: CrossPackageIncremental,
    // MerkleStore for future per-function caching (Phase 4 integration)
    #[allow(dead_code)]
    merkle: Arc<MerkleStore>,
    artifact_mgr: ArtifactManager,
    remote_store: Option<RemoteContentStore>,
    root_package: String,
    temp_dirs: Vec<tempfile::TempDir>,
    report: OrchestratorReport,
}

impl PackageGraphOrchestrator {
    pub fn new(root: &Path, config: OrchestratorConfig) -> Result<Self, String> {
        let graph = crate::graph::PackageGraph::discover(root)
            .map_err(|e| e.to_string())?;
        let cross_state = CrossPackageIncremental::load(root).unwrap_or_default();

        let root_package = graph.build_order()
            .map(|order| order.last().map(|n| n.name.clone()).unwrap_or_default())
            .unwrap_or_default();

        let cas_dir = root.join(".glyim/cas");
        let local_store = LocalContentStore::new(&cas_dir)
            .map_err(|e| format!("CAS init: {e}"))?;
        let store: Arc<dyn ContentStore> = Arc::new(local_store);
        let merkle = Arc::new(MerkleStore::new(store.clone()));
        let artifact_mgr = ArtifactManager::new(store, merkle.clone());

        let remote_store = if let Some(ref url) = config.remote_cache_url {
            let local_dir = root.join(".glyim/cas-remote");
            let remote_config = RemoteStoreConfig {
                endpoint: url.clone(),
                auth_token: config.remote_cache_token.clone(),
                local_dir,
            };
            RemoteContentStore::new(&remote_config).ok()
        } else {
            None
        };

        Ok(Self {
            workspace_root: root.to_path_buf(),
            graph,
            config,
            cross_state,
            merkle,
            artifact_mgr,
            remote_store,
            root_package,
            temp_dirs: Vec::new(),
            report: OrchestratorReport::default(),
        })
    }

    pub fn build(&mut self) -> Result<PathBuf, OrchestratorError> {
        let start = Instant::now();
        let order = self.graph.build_order().map_err(|e| OrchestratorError::Graph(e.to_string()))?;
        let root_name = order.last().map(|n| n.name.clone()).unwrap_or_default();
        let mut compiled_objects: Vec<PathBuf> = Vec::new();
        let pkg_names: Vec<String> = order.iter().map(|p| p.name.clone()).collect();

        for pkg_name in pkg_names {
            let pkg_start = Instant::now();
            eprintln!("  Compiling {} ...", pkg_name);

            let artifact_hash = self.cross_state.get_package_root(&pkg_name);
            // Compute current source hash to detect changes
            let pkg_node = self.graph.get(&pkg_name).unwrap();
            let source_path = pkg_node.dir.join("src/main.g");
            let current_source_hash = std::fs::read_to_string(&source_path)
                .ok()
                .map(|s| ContentHash::of(s.as_bytes()))
                .unwrap_or(ContentHash::ZERO);
            let source_changed = artifact_hash.map(|h| h != current_source_hash).unwrap_or(true);
            // Check if any dependency changed
            let deps_changed = pkg_node.manifest.dependencies.keys().any(|dep_name| {
                let dep_hash = self.cross_state.get_package_root(dep_name)
                    .unwrap_or(ContentHash::ZERO);
                self.cross_state.did_dependency_change(&pkg_name, dep_name, dep_hash)
            });

            // Try remote pull if not found locally
            if artifact_hash.is_some() && !self.config.force_rebuild {
                let hash = artifact_hash.unwrap();
                let local_exists = self.artifact_mgr.retrieve_object_code(hash).is_some();
                if !local_exists {
                    if let Some(ref remote) = self.remote_store {
                        if let Some(remote_data) = remote.retrieve(hash) {
                            self.artifact_mgr.store_object_code(&remote_data);
                            self.report.artifacts_pulled += 1;
                        }
                    }
                }
            }

            let should_skip = !self.config.force_rebuild
                && !source_changed
                && !deps_changed
                && artifact_hash.is_some()
                && {
                    let hash = artifact_hash.unwrap();
                    let name = format!("artifact:{}", hash);
                    if let Some(art_blob_hash) = self.artifact_mgr.resolve_name(&name) {
                        self.artifact_mgr.retrieve_object_code(art_blob_hash).is_some()
                    } else {
                        false
                    }
                };

            if should_skip {
                eprintln!("    (using cached artifact)");
                self.report.packages_cached.push(pkg_name.clone());
                if let Some(art) = {
                    let name = format!("artifact:{}", artifact_hash.unwrap());
                    self.artifact_mgr.resolve_name(&name)
                        .and_then(|h| self.artifact_mgr.retrieve_package_artifact(h))
                } {
                    match self.artifact_mgr.extract_object_code(&art) {
                        Ok(path) => {
                            compiled_objects.push(path);
                            self.report.per_package_timing.push((pkg_name, pkg_start.elapsed()));
                            continue;
                        }
                        Err(e) => {
                            eprintln!("    warning: cached object extraction failed: {e}");
                        }
                    }
                }
            }

            let pkg_node_clone = pkg_node.clone();
            match self.compile_single_package(&pkg_node_clone) {
                Ok(obj_path) => {
                    compiled_objects.push(obj_path);
                    self.report.packages_compiled.push(pkg_name.clone());
                }
                Err(e) => {
                    self.report.packages_failed.push(pkg_name.clone());
                    eprintln!("    FAILED: {e}");
                    return Err(e);
                }
            }
            self.report.per_package_timing.push((pkg_name, pkg_start.elapsed()));
        }

        let output_dir = self.workspace_root.join("target");
        std::fs::create_dir_all(&output_dir).ok();
        let output_path = output_dir.join(&root_name)
            .with_extension(if cfg!(target_os = "windows") { "exe" } else { "" });
        let linker_config = LinkConfig {
            target_triple: self.config.target.clone(),
            ..Default::default()
        };
        link_multi_object(&compiled_objects, &output_path, &linker_config)
            .map_err(|e| OrchestratorError::Link(e.to_string()))?;

        self.temp_dirs.clear();
        self.cross_state.save(&self.workspace_root).ok();
        self.report.total_elapsed = start.elapsed();
        Ok(output_path)
    }

    fn compile_single_package(&mut self, pkg: &PackageNode) -> Result<PathBuf, OrchestratorError> {
        let source_path = pkg.dir.join("src/main.g");
        let source = std::fs::read_to_string(&source_path).map_err(OrchestratorError::Io)?;
        let source_hash = ContentHash::of(source.as_bytes());

        let parsed = glyim_parse::parse(&source);
        if !parsed.errors.is_empty() {
            return Err(OrchestratorError::Parse(
                parsed.errors.into_iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"),
            ));
        }
        let mut interner = parsed.interner;
        let hir = glyim_hir::lower(&parsed.ast, &mut interner);
        let mut tc = glyim_typeck::TypeChecker::new(interner.clone());
        tc.check(&hir).map_err(|type_errors| {
            OrchestratorError::TypeCheck(
                type_errors.into_iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"),
            )
        })?;

        let context = inkwell::context::Context::create();
        let is_root = pkg.name == self.root_package;
        let mut builder = glyim_codegen_llvm::CodegenBuilder::new(
            &context,
            interner.clone(),
            tc.expr_types.clone(),
        );
        if !is_root {
            builder = builder.with_library_mode();
        }
        let mut codegen = builder.build().map_err(OrchestratorError::Codegen)?;
        codegen.generate(&hir).map_err(OrchestratorError::Codegen)?;

        let tmp_dir = tempfile::tempdir().map_err(OrchestratorError::Io)?;
        let obj_path = tmp_dir.path().join("output.o");
        codegen.write_object_file(&obj_path).map_err(OrchestratorError::Codegen)?;

        // Keep temp_dir alive for linking
        let obj_path_clone = obj_path.clone();
        self.temp_dirs.push(tmp_dir);

        let obj_bytes = std::fs::read(&obj_path_clone).map_err(OrchestratorError::Io)?;
        let obj_hash = self.artifact_mgr.store_object_code(&obj_bytes);
        let artifact = PackageArtifact {
            package_name: pkg.name.clone(),
            version: pkg.manifest.package.version.clone(),
            merkle_root: source_hash,
            symbol_table_hash: ContentHash::of(b"placeholder"),
            object_code_hash: obj_hash,
            per_fn_objects: Vec::new(),
            metadata_hash: ContentHash::of(b"0.5.0"),
            target_triple: self.config.target.clone(),
            opt_level: match self.config.mode {
                BuildMode::Debug => "debug".into(),
                BuildMode::Release => "release".into(),
            },
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        let artifact_hash = self.artifact_mgr.store_package_artifact(&artifact);
        self.cross_state.update_package_root(&pkg.name, source_hash);
        // Map source hash to artifact hash for quick cache lookups
        self.artifact_mgr.register_name(&format!("artifact:{}", source_hash), artifact_hash);
        self.cross_state.save(&self.workspace_root).ok();

        // Compute and store DependencyInterface
        let dep_iface = DependencyInterface::from_hir(&hir, &pkg.name, &pkg.manifest.package.version, &interner);
        let iface_bytes = dep_iface.to_bytes();
        let iface_hash = ContentHash::of(&iface_bytes);
        self.artifact_mgr.store_object_code(&iface_bytes);
        self.cross_state.record_dep_fingerprint(&pkg.name, "interface", iface_hash);

        // Push to remote if configured
        if let Some(ref remote) = self.remote_store {
            if remote.store(&obj_bytes) == obj_hash {
                let _ = remote.store_action_result(artifact_hash, ActionResult {
                    output_files: vec![],
                    exit_code: 0,
                    stdout_hash: None,
                    stderr_hash: None,
                });
                self.report.artifacts_pushed += 1;
            }
        }

        Ok(obj_path_clone)
    }

    pub fn check(&mut self) -> Result<(), OrchestratorError> {
        let order = self.graph.build_order().map_err(|e| OrchestratorError::Graph(e.to_string()))?;
        let pkg_names: Vec<String> = order.iter().map(|p| p.name.clone()).collect();
        for pkg_name in pkg_names {
            let pkg = self.graph.get(&pkg_name).unwrap().clone();
            self.compile_single_package(&pkg)?;
        }
        Ok(())
    }

    pub fn run(&mut self) -> Result<i32, OrchestratorError> {
        let order = self.graph.build_order().map_err(|e| OrchestratorError::Graph(e.to_string()))?;
        let main_pkg = order.last().ok_or(OrchestratorError::Graph("no packages".into()))?;
        let source_path = main_pkg.dir.join("src/main.g");
        let source = std::fs::read_to_string(&source_path).map_err(OrchestratorError::Io)?;
        glyim_compiler::pipeline::run_jit(&source)
            .map_err(|e| OrchestratorError::Pipeline(format!("{e:?}")))
    }

    pub fn report(&self) -> &OrchestratorReport { &self.report }
}

#[derive(Debug)]
pub enum OrchestratorError {
    Io(std::io::Error),
    Parse(String),
    TypeCheck(String),
    Codegen(String),
    Graph(String),
    Link(String),
    Pipeline(String),
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
            Self::TypeCheck(msg) => write!(f, "type error: {msg}"),
            Self::Codegen(msg) => write!(f, "codegen error: {msg}"),
            Self::Graph(msg) => write!(f, "graph error: {msg}"),
            Self::Link(msg) => write!(f, "link error: {msg}"),
            Self::Pipeline(msg) => write!(f, "pipeline error: {msg}"),
        }
    }
}
