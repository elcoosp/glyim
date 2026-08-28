use glyim_diag::GlyimDiagnostic;
use glyim_span::FileId;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Default)]
/// CompileOutput.
pub struct CompileOutput {
/// Struct.
    pub diagnostics: Vec<GlyimDiagnostic>,
/// Struct.
    pub syntax_tree: Option<glyim_syntax::SyntaxNode>,
/// Struct.
    pub def_map: Option<glyim_def_map::CrateDefMap>,
/// Struct.
    pub typeck_result: Option<glyim_typeck::TypeckResult>,
/// Struct.
    pub mir_bodies: Vec<Arc<glyim_mir::Body>>,
/// Struct.
    pub ty_ctx: Option<Arc<glyim_type::TyCtx>>,
    /// Path to a linked executable, populated only when compilation succeeded
    /// AND the produced object file was successfully linked (Tier 7.2). `None`
    /// otherwise (e.g. a mock backend that emits no real object, or a link
    /// failure) — in which case run-pass/run-fail strategies report "no
    /// executable produced" rather than silently running nothing.
    pub executable_path: Option<PathBuf>,
}

impl std::fmt::Debug for CompileOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompileOutput")
            .field("diagnostics", &self.diagnostics)
            .field("syntax_tree", &self.syntax_tree)
            .field("def_map", &self.def_map)
            .field("typeck_result", &self.typeck_result)
            .field("mir_bodies", &self.mir_bodies)
            .field("ty_ctx", &self.ty_ctx.as_ref().map(|_| "TyCtx"))
            .field("executable_path", &self.executable_path)
            .finish()
    }
}

/// TestCompiler.
pub trait TestCompiler: Send + Sync {
/// compile.
    fn compile(&self, source: &str, file_id: FileId, flags: &[String]) -> CompileOutput;
}

/// FrontendOnlyCompiler.
pub struct FrontendOnlyCompiler;

impl TestCompiler for FrontendOnlyCompiler {
    fn compile(&self, source: &str, file_id: FileId, _flags: &[String]) -> CompileOutput {
        tracing::info!(phase = "parse", file_id = file_id.to_raw());
        let result = glyim_frontend::parse_to_syntax(source, file_id);
        CompileOutput {
            diagnostics: result.diagnostics,
            syntax_tree: Some(result.root),
            def_map: None,
            typeck_result: None,
            mir_bodies: Vec::new(),
            ty_ctx: None,
            executable_path: None,
        }
    }
}

/// PipelineCompiler.
pub struct PipelineCompiler {
    backend: Arc<dyn glyim_codegen::CodegenBackend + Send + Sync>,
    /// When true (and the `real-llvm` feature is compiled in), the compiler
    /// uses the real `LlvmBackend` and links a runnable native executable,
    /// turning run-pass fixtures into a true runtime proof. Otherwise it falls
    /// back to `MockCodegen` (no object emitted), which is the default so the
    /// harness stays hermetic on hosts without the LLVM toolchain.
    real_llvm: bool,
    /// Optional procedural-macro registry (Phase 9.2). When set, macro
    /// expansion runs this registry's `MacroKind::Proc` invocations through the
    /// loaded cdylib functions during the compile's expansion stage. `None`
    /// disables proc-macro dispatch (declarative + builtin macros still run).
    proc_registry: Option<Arc<glyim_proc_macro::Registry>>,
}

impl PipelineCompiler {
/// new.
    pub fn new(backend: Arc<dyn glyim_codegen::CodegenBackend + Send + Sync>) -> Self {
        Self {
            backend,
            real_llvm: false,
            proc_registry: None,
        }
    }

    /// Enable the real `LlvmBackend` path: produce a real native object,
    /// link it, and execute run-pass fixtures on the host. Requires the
    /// `real-llvm` feature (which pulls in `glyim-codegen-llvm`). When the
    /// feature is absent this is a no-op and the mock backend is used.
    ///
    /// NOTE: the native async codegen path is currently broken (tracked gap —
    /// `glyim-codegen-llvm` panics in `fn_abi_of` for monomorphized
    /// `block_on<F>`/`poll` types), so enabling this on the M5 async fixtures
    /// surfaces a genuine failure rather than a green run. That is the intended
    /// strict behavior: the CI job must not pass by tolerating a codegen gap.
    pub fn with_real_llvm(mut self) -> Self {
        self.real_llvm = true;
        self
    }

    /// Like [`with_real_llvm`](Self::with_real_llvm) but only flips the flag
    /// when `enabled` is true — convenient for env-var gating without
    /// branching at every call site.
    pub fn with_real_llvm_if(mut self, enabled: bool) -> Self {
        self.real_llvm = enabled;
        self
    }

    /// Inject a procedural-macro [`Registry`](glyim_proc_macro::Registry) so
    /// proc-macro calls are expanded during compilation (Phase 9.2
    /// two-stage host compile: the registry is populated either in-process for
    /// tests or by `load_cdylib` from a compiled proc-macro crate).
    pub fn with_proc_registry(mut self, registry: Option<Arc<glyim_proc_macro::Registry>>) -> Self {
        self.proc_registry = registry;
        self
    }
}

impl TestCompiler for PipelineCompiler {
    fn compile(&self, source: &str, file_id: FileId, _flags: &[String]) -> CompileOutput {
        use glyim_db::{CrateConfig, Database};

        tracing::info!(phase = "full-pipeline", file_id = file_id.to_raw());

        // Each `compile()` call writes its source to a temp file that the
        // pipeline reads back. The path must be UNIQUE per call: many tests
        // run concurrently and several reuse the same `FileId` (e.g. `1`), so
        // a path derived only from `file_id` collides, causing one thread to
        // overwrite another's source and the compiler to read the wrong
        // program ("MIR body not found"). We uniquify with a process-global
        // counter combined with the pid.
        static CALL_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let call_id = CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let unique_tag = format!("{}_{}", std::process::id(), call_id);

        let config = CrateConfig {
            name: format!("test_{}", file_id.to_raw()),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            opt_level: 0,
        };

        let mut db = Database::new(config);
        let path = std::env::temp_dir().join(format!("glyim_test_{}_{}.g", unique_tag, file_id.to_raw()));
        std::fs::write(&path, source).expect("failed to write temp source file for PipelineCompiler");
        db.vfs().add_file_content(&path, Arc::from(source));

        // Phase 9.2: run macro expansion over the parsed source *before* the
        // rest of the pipeline consumes it. Declarative + builtin macros always
        // run; proc-macro calls dispatch through the injected registry (the
        // two-stage load_cdylib populates it from a compiled cdylib). The
        // expanded program is pushed back into the VFS (the pipeline reads
        // source from the VFS, not disk) so `compile_file_with_artifacts`
        // re-parses the macro-free source.
        let initial_parse = glyim_frontend::parse_to_syntax(source, file_id);
        let mut expansion_diags: Vec<GlyimDiagnostic> = initial_parse.diagnostics.clone();
        if initial_parse.diagnostics.is_empty() {
            let mut hygiene = glyim_span::HygieneCtx::new();
            let mut expander = glyim_meta::Expander::new(&mut hygiene);
            if let Some(reg) = self.proc_registry.as_ref() {
                expander.with_proc_registry(Some(reg.as_ref()));
            }
            let (expanded, mut diags) = expander.expand_crate(&initial_parse.root);
            expansion_diags.append(&mut diags);
            // Push the expanded program back to BOTH the VFS and the on-disk
            // file: `compile_file_with_artifacts` re-reads the source from disk
            // (via `add_file_from_disk`), so the disk copy must also reflect the
            // macro-free form for the pipeline to consume it. `expand_crate`
            // emits a whitespace-free token stream; re-serialize it with
            // separators (walking the green token stream so token boundaries
            // are preserved) so rowan reparses it faithfully (Phase 9.2).
            let expanded_src = glyim_meta::join_tokens_with_spaces(&expanded);
            db.vfs().add_file_content(&path, Arc::from(expanded_src.clone()));
            std::fs::write(&path, &expanded_src)
                .expect("failed to write expanded source for PipelineCompiler");
        }

        let output_path = std::env::temp_dir().join(format!("glyim_test_{}_{}.o", unique_tag, file_id.to_raw()));
        let ty_ctx = db.get_ty_ctx();
        let exe_path = output_path.with_extension("");

        // Choose the backend. By default (and when the `real-llvm` feature is
        // not compiled in) we use the injected `MockCodegen`, which emits no
        // object so run-pass fixtures cannot link — keeping the harness
        // hermetic on hosts without the LLVM toolchain. When `real_llvm` is
        // enabled *and* the feature is present, we drive the real
        // `LlvmBackend`, targeting `x86_64-unknown-linux-gnu` and emitting a
        // C-ABI `main` entry symbol so the produced object links into a
        // runnable ELF. The `entry_main` local id is discovered via a
        // lightweight parse+def-map pre-pass (mirroring `glyip`).
        //
        // NOTE: `LlvmBackend` owns an `inkwell::Context`, which is `!Sync`, so
        // it cannot live inside the `Arc<dyn ... + Sync>` shared across the
        // rayon worker threads. We therefore construct it as a *local* value
        // per `compile()` call and pass `&backend` (a `&dyn CodegenBackend`)
        // straight into the pipeline — each worker thread gets its own backend,
        // which is exactly what LLVM requires.
        #[cfg(feature = "real-llvm")]
        let real_backend_local: Option<glyim_codegen_llvm::LlvmBackend> = if self.real_llvm {
            use glyim_pipeline::Pipeline;
            let mut real = glyim_codegen_llvm::LlvmBackend::with_db(&db)
                .with_target("x86_64-unknown-linux-gnu");
            if let Some(main_id) = Pipeline::entry_main_local_id(&mut db, &path) {
                real = real.with_entry_main(main_id);
            }
            Some(real)
        } else {
            None
        };
        #[cfg(feature = "real-llvm")]
        let backend: &dyn glyim_codegen::CodegenBackend = match &real_backend_local {
            Some(real) => real,
            None => &*self.backend,
        };
#[cfg(not(feature = "real-llvm"))]
        let backend: &dyn glyim_codegen::CodegenBackend = &*self.backend;

        match glyim_pipeline::Pipeline::compile_file_with_artifacts(
            &mut db,
            &path,
            backend,
            &output_path,
            None,
            None,
        ) {
            Ok(artifacts) => {
                // Tier 7.2: link the produced object file into a real executable
                // so run-pass/run-fail strategies can actually execute it. The
                // linker is only invoked when codegen produced a real object
                // (the production LLVM backend does; a mock backend that emits
                // nothing will fail to link and we fall back to `None`).
                let executable_path = glyim_cli::linker::invoke_linker(&output_path, &exe_path, None, None, None)
                    .ok()
                    .map(|()| exe_path.clone());
                CompileOutput {
                    diagnostics: expansion_diags,
                    syntax_tree: None,
                    def_map: Some(artifacts.def_map),
                    typeck_result: Some(artifacts.typeck_result),
                    mir_bodies: artifacts.mir_bodies,
                    ty_ctx: Some(artifacts.ty_ctx),
                    executable_path,
                }
            }
            Err(mut diags) => {
                expansion_diags.append(&mut diags);
                CompileOutput {
                    diagnostics: expansion_diags,
                    syntax_tree: None,
                    def_map: None,
                    typeck_result: None,
                    mir_bodies: Vec::new(),
                    ty_ctx,
                    executable_path: None,
                }
            }
        }
    }
}
