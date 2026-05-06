use glyim_compiler::pipeline::BuildMode;
use glyim_merkle::MerkleStore;
use glyim_macro_vfs::{ContentHash, ContentStore, LocalContentStore, RemoteContentStore, RemoteStoreConfig};
use crate::graph::{PackageGraph, PackageNode};
use crate::artifacts::ArtifactManager;
use crate::incremental::CrossPackageIncremental;
use crate::linker::{LinkConfig, link_multi_object};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

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


#[allow(dead_code)]

#[allow(dead_code)]
pub struct PackageGraphOrchestrator {
    workspace_root: PathBuf,
    graph: PackageGraph,
    config: OrchestratorConfig,
    cross_state: CrossPackageIncremental,
    merkle: Arc<MerkleStore>,
    artifact_mgr: ArtifactManager,
    remote_store: Option<RemoteContentStore>,
    report: OrchestratorReport,
}

impl PackageGraphOrchestrator {
    pub fn new(root: &Path, config: OrchestratorConfig) -> Result<Self, String> {
        let graph = crate::graph::PackageGraph::discover(root)
            .map_err(|e| e.to_string())?;
        let cross_state = CrossPackageIncremental::load(root).unwrap_or_default();

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
            // Try remote pull first if configured and not found locally
            if artifact_hash.is_some() && !self.config.force_rebuild {
                let hash = artifact_hash.unwrap();
                let local_exists = self.artifact_mgr.retrieve_object_code(hash).is_some();
                if !local_exists {
                    if let Some(ref remote) = self.remote_store {
                        if let Some(remote_data) = remote.retrieve(hash) {
                            // Store locally for future use
                            self.artifact_mgr.store_object_code(&remote_data);
                            if let Some(art) = remote.retrieve_action_result(hash).or_else(|| {
                                // reconstruct PackageArtifact from remote? For now, skip
                                None
                            }) {
                                // Not needed for basic object code pull
                            }
                            self.report.artifacts_pulled += 1;
                        }
                    }
                }
            }
            let should_skip = !self.config.force_rebuild
                && artifact_hash.is_some()
                && self.artifact_mgr.retrieve_object_code(artifact_hash.unwrap()).is_some();

            if should_skip {
                eprintln!("    (using cached artifact)");
                self.report.packages_cached.push(pkg_name.clone());
                // Retrieve artifact and extract object code
                if let Some(art) = self.artifact_mgr.retrieve_package_artifact(artifact_hash.unwrap()) {
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

            let pkg_node = self.graph.get(&pkg_name).unwrap().clone();
            match self.compile_single_package(&pkg_node) {
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

        self.cross_state.save(&self.workspace_root).ok();
        self.report.total_elapsed = start.elapsed();
        Ok(output_path)
    }

    fn compile_single_package(&mut self, pkg: &PackageNode) -> Result<PathBuf, OrchestratorError> {
        let source_path = pkg.dir.join("src/main.g");
        let source = std::fs::read_to_string(&source_path).map_err(OrchestratorError::Io)?;
        // Compute source hash for cache key
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
        let mut codegen = glyim_codegen_llvm::CodegenBuilder::new(
            &context,
            interner,
            tc.expr_types.clone(),
        )
        .build()
        .map_err(OrchestratorError::Codegen)?;
        codegen.generate(&hir).map_err(OrchestratorError::Codegen)?;

        let tmp_dir = tempfile::tempdir().map_err(OrchestratorError::Io)?;
        let obj_path = tmp_dir.path().join("output.o");
        codegen.write_object_file(&obj_path).map_err(OrchestratorError::Codegen)?;

        // Store object code in CAS and update cross-package state
        let obj_bytes = std::fs::read(&obj_path).map_err(OrchestratorError::Io)?;
        let obj_hash = self.artifact_mgr.store_object_code(&obj_bytes);
        // Create package artifact (simplified; full version would include symbol table)
        let artifact = crate::artifacts::PackageArtifact {
            package_name: pkg.name.clone(),
            version: pkg.manifest.package.version.clone(),
            merkle_root: source_hash,  // use source hash as root for now
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
        self.cross_state.save(&self.workspace_root).ok();

        // Push to remote if configured
        if let Some(ref remote) = self.remote_store {
            if remote.store(&obj_bytes) == obj_hash {
                if let Err(e) = remote.store_action_result(artifact_hash, glyim_macro_vfs::ActionResult {
                    output_files: vec![],
                    exit_code: 0,
                    stdout_hash: None,
                    stderr_hash: None,
                }) {
                    eprintln!("    warning: remote push failed: {e}");
                } else {
                    self.report.artifacts_pushed += 1;
                }
            }
        }

        Ok(obj_path)
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

    pub fn report(&self) -> &OrchestratorReport {
        &self.report
    }
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
