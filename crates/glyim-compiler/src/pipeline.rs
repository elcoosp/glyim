use glyim_codegen_llvm::codegen::CoverageMode;
use glyim_codegen_llvm::runtime_shims;
use glyim_codegen_llvm::{Codegen, CodegenBuilder, compile_to_ir};
use glyim_diag::diagnostic::Diagnostic;
use glyim_hir::ExprId;
use glyim_hir::types::HirType;
use glyim_interner::Interner;
use glyim_profiler::ProfileCollector;
use glyim_profiler::StageName;
use glyim_query::fingerprint::Fingerprint;
use glyim_query::incremental::IncrementalState;
use glyim_typeck::TypeChecker;
use glyim_typeck::TypeError;
use inkwell::OptimizationLevel;
use inkwell::context::Context;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::{fs, process::Command};
/// The build mode for compilation.
///
/// # Stability
/// *Stable.*
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildMode {
    #[default]
    Debug,
    Release,
}
impl BuildMode {
    pub fn opt_level(&self) -> inkwell::OptimizationLevel {
        match self {
            BuildMode::Debug => inkwell::OptimizationLevel::None,
            BuildMode::Release => inkwell::OptimizationLevel::Aggressive,
        }
    }
    pub fn is_release(&self) -> bool {
        matches!(self, BuildMode::Release)
    }
}
#[derive(Debug)]
#[must_use]
pub enum PipelineError {
    Io(std::io::Error),
    Parse(Vec<glyim_parse::ParseError>),
    Codegen(String),
    TypeCheck(Vec<TypeError>),
    Link(String),
    Run(std::io::Error),
    Manifest(crate::manifest::ManifestError),
    MissingSysroot(String),
    Diagnostics(Vec<Diagnostic>),
}
impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Parse(errs) => {
                writeln!(f, "{} parse error(s):", errs.len())?;
                for e in errs {
                    writeln!(f, "  - {e}")?;
                }
                Ok(())
            }
            Self::Codegen(msg) => write!(f, "codegen error: {msg}"),
            Self::TypeCheck(errs) => {
                writeln!(f, "{} type error(s):", errs.len())?;
                for e in errs {
                    writeln!(f, "  - {e}")?;
                }
                Ok(())
            }
            Self::Link(msg) => write!(f, "linker error: {msg}"),
            Self::Run(e) => write!(f, "execution error: {e}"),
            Self::Manifest(e) => write!(f, "manifest error: {e}"),
            Self::MissingSysroot(msg) => write!(f, "sysroot error: {msg}"),
            Self::Diagnostics(diags) => {
                for d in diags {
                    writeln!(f, "{}: {}", d.severity, d.message)?;
                }
                Ok(())
            }
        }
    }
}
impl std::error::Error for PipelineError {}
impl From<std::io::Error> for PipelineError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<String> for PipelineError {
    fn from(s: String) -> Self {
        Self::Codegen(s)
    }
}
static CUSTOM_ASSERT_FN: Mutex<Option<unsafe extern "C" fn(*const u8, i64)>> = Mutex::new(None);
static CUSTOM_ABORT_FN: Mutex<Option<unsafe extern "C" fn()>> = Mutex::new(None);
#[allow(deprecated)]
pub fn set_jit_abort_handler(handler: unsafe extern "C" fn()) {
    *CUSTOM_ABORT_FN.lock().unwrap() = Some(handler);
}
pub fn set_jit_assert_handler(handler: unsafe extern "C" fn(*const u8, i64)) {
    *CUSTOM_ASSERT_FN.lock().unwrap() = Some(handler);
}
fn detect_no_std(source: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed == "no_std" {
            return true;
        }
    }
    false
}

fn is_manifest_no_std(manifest: &glyim_pkg::manifest::PackageManifest) -> bool {
    manifest.package.no_std == Some(true)
}

fn load_source_with_prelude(input: &Path) -> Result<(String, bool), PipelineError> {
    let source = format!("{}\n{}", PRELUDE, fs::read_to_string(input)?);
    let is_no_std = detect_no_std(&source);
    Ok((source, is_no_std))
}
fn load_source_with_prelude_opt(
    input: &Path,
    force_no_std: Option<bool>,
) -> Result<(String, bool), PipelineError> {
    let source = fs::read_to_string(input)?;
    let is_no_std = force_no_std.unwrap_or_else(|| detect_no_std(&source));
    let final_source = if is_no_std {
        source
    } else {
        format!("{}\n{}", PRELUDE, source)
    };
    Ok((final_source, is_no_std))
}

// ── shared monomorphize→merged_types helper ──────────────────────
pub(crate) fn merge_mono_types(
    hir: &glyim_hir::Hir,
    interner: &mut Interner,
    expr_types: &[HirType],
    call_type_args: &std::collections::HashMap<ExprId, Vec<HirType>>,
) -> (Vec<HirType>, glyim_hir::Hir) {
    let mono_result =
        glyim_hir::monomorphize::monomorphize(hir, interner, expr_types, call_type_args);
    let mut merged = expr_types.to_vec();
    for (id, ty) in &mono_result.type_overrides {
        if id.as_usize() < merged.len() {
            merged[id.as_usize()] = ty.clone();
        } else {
            merged.resize(id.as_usize() + 1, HirType::Never);
            merged[id.as_usize()] = ty.clone();
        }
    }
    // Generic → Named fallback
    for ty in &mut merged {
        if let HirType::Generic(sym, args) = ty {
            let all_concrete = args.iter().all(|a| match a {
                HirType::Named(s) => {
                    let s = interner.resolve(*s);
                    !(s.len() == 1 && s.chars().next().unwrap().is_uppercase())
                }
                _ => true,
            });
            if all_concrete && !args.is_empty() {
                let mangled = glyim_hir::monomorphize::mangle_type_name(interner, *sym, args);
                *ty = HirType::Named(mangled);
            } else {
                *ty = HirType::Named(*sym);
            }
        }
    }
    (merged, mono_result.hir)
}

/// Compile a Glyim source file into an executable binary.
///
/// # Stability
/// *Stable.*
#[allow(deprecated)]
#[tracing::instrument(name = "build", skip_all)]
pub fn build(
    input: &Path,
    output: Option<&Path>,
    target: Option<&str>,
) -> Result<PathBuf, PipelineError> {
    ProfileCollector::enter_stage(StageName::Parse);
    let (source, _) = load_source_with_prelude(input)?;
    let config = PipelineConfig {
        target: target.map(|s| s.to_string()),
        ..Default::default()
    };
    let compiled = compile_source_to_hir(source, input, &config)?;
    let output_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let stem = input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        PathBuf::from(stem)
    });
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    let context = Context::create();
    let mut codegen = Codegen::with_line_tables(
        &context,
        compiled.interner.clone(),
        compiled.merged_types.clone(),
        compiled.source.clone(),
        &input.to_string_lossy(),
    )
    .map_err(PipelineError::Codegen)?;
    if let Some(t) = &config.target {
        crate::cross::validate_target(t).map_err(PipelineError::Codegen)?;
        codegen = codegen.with_target(t);
    }
    if compiled.is_no_std {
        codegen = codegen.with_no_std();
    }
    codegen
        .generate(&compiled.mono_hir)
        .map_err(PipelineError::Codegen)?;
    debug_ir(&codegen);
    codegen
        .write_object_file(&obj_path)
        .map_err(PipelineError::Codegen)?;
    link_object(&obj_path, &output_path, false)?;
    ProfileCollector::exit_stage(StageName::Parse, 1, 0, 0);
    Ok(output_path)
}
const PRELUDE: &str = "\
pub enum Option<T> {
    Some(T),
    None,
}
pub enum Result<T, E> {
    Ok(T),
    Err(E),
}
extern {
    fn __glyim_alloc(size: i64) -> *mut u8;
    fn __glyim_free(ptr: *mut u8);
    fn __glyim_hash_bytes(data: *const u8, len: i64) -> i64;
    fn __glyim_hash_seed() -> i64;
    fn abort();
}
";

/// Configuration for the compilation pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub mode: BuildMode,
    pub target: Option<String>,
    pub force_no_std: Option<bool>,
    pub jit_mode: bool,
    pub coverage_mode: CoverageMode,
    pub coverage_output: Option<std::path::PathBuf>,
    pub cas_dir: std::path::PathBuf,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            mode: BuildMode::Debug,
            target: None,
            force_no_std: None,
            jit_mode: false,
            coverage_mode: CoverageMode::Off,
            coverage_output: None,
            cas_dir: dirs_next::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from(".glyim/cas")),
        }
    }
}

#[allow(dead_code)]
pub struct CompiledHir {
    pub hir: glyim_hir::Hir,
    pub mono_hir: glyim_hir::Hir,
    pub merged_types: Vec<glyim_hir::types::HirType>,
    pub interner: glyim_interner::Interner,
    pub source: String,
    pub is_no_std: bool,
}

#[deprecated(note = "Use QueryPipeline::compile() instead")]
pub(crate) fn compile_source_to_hir(
    source: String,
    input_path: &std::path::Path,
    config: &PipelineConfig,
) -> Result<CompiledHir, PipelineError> {
    let is_no_std = config
        .force_no_std
        .unwrap_or_else(|| detect_no_std(&source));

    let cas_dir = &config.cas_dir;
    let source = crate::macro_expand::expand_macros(
        &source,
        input_path.parent().unwrap_or(std::path::Path::new(".")),
        cas_dir,
    )
    .unwrap_or(source);

    let parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Diagnostics(
            parse_out.errors.into_iter().map(|e| e.into()).collect(),
        ));
    }
    let mut interner = parse_out.interner;

    let decl_output = glyim_parse::declarations::parse_declarations(&source);
    let decl_table =
        glyim_hir::decl_table::DeclTable::from_declarations(&decl_output.ast, &mut interner);

    let mut hir = glyim_hir::lower_with_declarations(&parse_out.ast, &mut interner, &decl_table);
    let mut typeck = glyim_typeck::TypeChecker::new(interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::Diagnostics(
            errs.into_iter().map(|e| e.into()).collect(),
        ));
    }
    glyim_hir::desugar_method_calls(&mut hir, &typeck.expr_types, &mut interner);

    let expr_types = typeck.expr_types.clone();
    let call_type_args = std::mem::take(&mut typeck.call_type_args);
    let (merged_types, mono_hir) =
        merge_mono_types(&hir, &mut interner, &expr_types, &call_type_args);

    Ok(CompiledHir {
        hir,
        mono_hir,
        merged_types,
        interner,
        source,
        is_no_std,
    })
}

fn execute_jit(
    compiled: &CompiledHir,
    mode: BuildMode,
    target: Option<&str>,
) -> Result<i32, PipelineError> {
    let context = Context::create();
    let mut codegen = match mode {
        BuildMode::Debug => Codegen::with_debug(
            &context,
            compiled.interner.clone(),
            compiled.merged_types.clone(),
            compiled.source.clone(),
            "jit",
        )
        .map_err(PipelineError::Codegen)?,
        BuildMode::Release => CodegenBuilder::new(
            &context,
            compiled.interner.clone(),
            compiled.merged_types.clone(),
        )
        .build()?,
    };
    if let Some(t) = target {
        crate::cross::validate_target(t).map_err(PipelineError::Codegen)?;
        codegen = codegen.with_target(t);
    }
    if compiled.is_no_std {
        codegen = codegen.with_no_std();
    }
    codegen = codegen.with_jit_mode();
    codegen
        .generate(&compiled.mono_hir)
        .map_err(PipelineError::Codegen)?;
    debug_ir(&codegen);
    let engine = codegen
        .get_module()
        .create_jit_execution_engine(mode.opt_level())
        .map_err(|e| PipelineError::Codegen(format!("JIT: {e}")))?;
    runtime_shims::map_runtime_shims_for_jit(
        &engine,
        codegen.get_module(),
        *CUSTOM_ASSERT_FN.lock().unwrap(),
        *CUSTOM_ABORT_FN.lock().unwrap(),
    );
    unsafe {
        let main_fn = engine
            .get_function::<unsafe extern "C" fn() -> i32>("main")
            .map_err(|e| PipelineError::Codegen(format!("JIT main: {e}")))?;
        let exit_code = main_fn.call();
        ProfileCollector::exit_stage(StageName::Codegen, 1, 0, 0);
        ProfileCollector::exit_stage(StageName::Parse, 1, 0, 0);
        Ok(exit_code)
    }
}

#[tracing::instrument(name = "run", skip_all)]
pub fn run(input: &Path, target: Option<&str>) -> Result<i32, PipelineError> {
    run_with_mode(input, BuildMode::Debug, target, None)
}
#[tracing::instrument(name = "check", skip_all)]
/// Compile source to HIR using the incremental query engine.
/// On first call this behaves identically to the standard path.
/// On subsequent calls it loads previous state, detects changes,
/// invalidates affected queries, and re-runs only the Red stages.
#[allow(dead_code)]
pub(crate) fn compile_source_to_hir_incremental(
    source: String,
    input_path: &std::path::Path,
    config: &PipelineConfig,
    cache_dir: &std::path::Path,
) -> Result<CompiledHir, PipelineError> {
    let mut state = IncrementalState::load_or_create(cache_dir);
    let source_fp = Fingerprint::of(source.as_bytes());
    let input_str = input_path.to_string_lossy().to_string();
    let _report = state.apply_changes(&[(&input_str, source_fp)]);
    let source_key = Fingerprint::combine(Fingerprint::of_str(&input_str), source_fp);
    if !state.ctx().is_green(&source_key) {
        tracing::info!("Incremental: source changed or new — recomputing");
    }
    let compiled = compile_source_to_hir(source, input_path, config)?;
    state.ctx().insert(
        source_key,
        std::sync::Arc::new(()),
        source_fp,
        vec![glyim_query::Dependency::file(&input_str, source_fp)],
    );
    state.record_source(&input_str, source_fp);
    if let Err(e) = state.save() {
        tracing::warn!("Failed to save incremental state: {e}");
    }
    Ok(compiled)
}

/// Build a project using the incremental query-driven pipeline.
/// Returns the output path and a detailed incremental report.
pub fn build_incremental(
    input: &Path,
    output: Option<&Path>,
    mode: BuildMode,
    target: Option<&str>,
) -> Result<(PathBuf, crate::queries::IncrementalReport), PipelineError> {
    use crate::queries::QueryPipeline;
    use glyim_macro_vfs::ContentHash;
    use glyim_merkle::{MerkleNode, MerkleNodeData, MerkleNodeHeader, compute_root_hash};

    let (source, _) = load_source_with_prelude(input)?;
    let cache_dir = input
        .parent()
        .unwrap_or(Path::new("."))
        .join(".glyim/incremental");

    let mut qp = QueryPipeline::new(
        &cache_dir,
        PipelineConfig {
            mode,
            target: target.map(String::from),
            coverage_mode: CoverageMode::Off, // incremental doesn't support coverage from CLI yet; will add later
            ..Default::default()
        },
    );

    // 1. Full compilation to get HIR + type info and per‑function fingerprints
    let compiled = qp.compile(&source, input)?;
    let fps: Vec<(String, glyim_query::Fingerprint)> =
        crate::queries::item_fingerprints(&compiled.hir, &compiled.interner);

    // 2. Compute Merkle root from fingerprints
    let merkle_items: Vec<(String, ContentHash)> = fps
        .iter()
        .map(|(name, fp)| (name.clone(), ContentHash::from_bytes(*fp.as_bytes())))
        .collect();
    let merkle_root = compute_root_hash(&merkle_items);

    // 3. Check for cached full object in Merkle store
    let mut obj_path_opt = None;
    if let Some(ref merkle) = qp.merkle_store
        && let Some(hash) = merkle.resolve_name(&format!("full-obj:{}", merkle_root.to_hex()))
        && let Some(node) = merkle.get(&hash)
        && let MerkleNodeData::ObjectCode { bytes, .. } = &node.data
    {
        let tmp_dir = tempfile::tempdir()?;
        let cached_obj = tmp_dir.path().join("cached.o");
        fs::write(&cached_obj, bytes)?;
        obj_path_opt = Some(cached_obj);
        // Keep tmp_dir alive
        Box::leak(Box::new(tmp_dir)); // leak to maintain lifetime (test binary only)
    }

    // 4. If no cached object, perform full codegen
    let obj_path = match obj_path_opt {
        Some(p) => p,
        None => {
            let tmp_dir = tempfile::tempdir()?;
            let obj_p = tmp_dir.path().join("output.o");
            let context = inkwell::context::Context::create();
            let cov_mode = CoverageMode::Off;
            let mut codegen = CodegenBuilder::new(
                &context,
                compiled.interner.clone(),
                compiled.merged_types.clone(),
            )
            .with_coverage_mode(cov_mode)
            .build()?;
            if let Some(t) = target {
                crate::cross::validate_target(t).map_err(PipelineError::Codegen)?;
                codegen = codegen.with_target(t);
            }
            if compiled.is_no_std {
                codegen = codegen.with_no_std();
            }
            codegen
                .generate(&compiled.mono_hir)
                .map_err(PipelineError::Codegen)?;
            codegen
                .write_object_file_with_opt(&obj_p, mode.opt_level())
                .map_err(PipelineError::Codegen)?;

            // Store full object in Merkle cache
            if let Some(ref merkle) = qp.merkle_store {
                let obj_bytes = fs::read(&obj_p)?;
                let node = MerkleNode {
                    hash: ContentHash::ZERO,
                    children: vec![],
                    data: MerkleNodeData::ObjectCode {
                        symbol_name: "full-module".to_string(),
                        bytes: obj_bytes,
                    },
                    header: MerkleNodeHeader {
                        data_type_tag: 0x04,
                        child_count: 0,
                    },
                };
                let hash = merkle.put(node);
                merkle.register_name(&format!("full-obj:{}", merkle_root.to_hex()), hash);
                merkle.flush();
            }
            Box::leak(Box::new(tmp_dir)); // keep alive
            obj_p
        }
    };

    let output_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let stem = input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        PathBuf::from(stem)
    });

    link_object(&obj_path, &output_path, mode == BuildMode::Release)?;

    let report = qp.report().clone();
    Ok((output_path, report))
}
pub fn semantic_source_hash(source: &str) -> glyim_macro_vfs::ContentHash {
    semantic_hash_of_source(source)
}

/// Compute a true semantic hash by parsing, lowering, normalizing, and hashing.
pub fn semantic_hash_of_source(source: &str) -> glyim_macro_vfs::ContentHash {
    use glyim_hir::lower as lower_fn;
    use glyim_hir::semantic_hash::semantic_hash_item;

    let parse_out = glyim_parse::parse(source);
    let mut interner = parse_out.interner;
    if interner.is_empty() || !parse_out.errors.is_empty() {
        return glyim_macro_vfs::ContentHash::of(source.as_bytes());
    }
    let hir = lower_fn(&parse_out.ast, &mut interner);
    let item_hashes: Vec<glyim_macro_vfs::ContentHash> = hir
        .items
        .iter()
        .map(|item| {
            let semantic = semantic_hash_item(item, &interner);
            glyim_macro_vfs::ContentHash::from_bytes(*semantic.as_bytes())
        })
        .collect();
    if item_hashes.is_empty() {
        return glyim_macro_vfs::ContentHash::of(b"empty_module");
    }
    for item in &hir.items {
        let _h = semantic_hash_item(item, &interner);
    }
    let mut combined = Vec::new();
    for h in &item_hashes {
        combined.extend_from_slice(h.as_bytes());
    }
    glyim_macro_vfs::ContentHash::of(&combined)
}

/// Execute the program using the bytecode interpreter (Tier-0).
/// Provides sub-millisecond feedback without LLVM compilation.
pub fn run_live(source: &str) -> Result<i32, PipelineError> {
    use glyim_bytecode::compiler::BytecodeCompiler;
    use glyim_bytecode::interpreter::BytecodeInterpreter;
    use glyim_bytecode::value::Value;
    let parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Diagnostics(
            parse_out.errors.into_iter().map(|e| e.into()).collect(),
        ));
    }
    let mut interner = parse_out.interner;
    let decl_output = glyim_parse::declarations::parse_declarations(source);
    let decl_table =
        glyim_hir::decl_table::DeclTable::from_declarations(&decl_output.ast, &mut interner);
    let hir = glyim_hir::lower_with_declarations(&parse_out.ast, &mut interner, &decl_table);
    let mut typeck = glyim_typeck::TypeChecker::new(interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::Diagnostics(
            errs.into_iter().map(|e| e.into()).collect(),
        ));
    }
    let mut compiler = BytecodeCompiler::new(&interner);
    let mut interpreter = BytecodeInterpreter::new();
    for item in &hir.items {
        if let glyim_hir::HirItem::Fn(hir_fn) = item
            && interner.resolve(hir_fn.name) == "main"
        {
            let bc_fn = compiler.compile_fn(hir_fn);
            let result = interpreter.execute_fn(&bc_fn, &[]);
            return match result {
                Value::Int(n) => Ok(n as i32),
                Value::Bool(true) => Ok(0),
                Value::Bool(false) => Ok(1),
                _ => Ok(0),
            };
        }
    }
    Err(PipelineError::Codegen("no 'main' function found".into()))
}

pub fn check(input: &Path) -> Result<(), PipelineError> {
    let mut source = format!("{}\n{}", PRELUDE, fs::read_to_string(input)?);

    // Expand macros before parsing (simple source transformation)
    let cas_dir = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from(".glyim/cas"));
    if let Ok(expanded) = crate::macro_expand::expand_macros(
        &source,
        input.parent().unwrap_or(Path::new(".")),
        &cas_dir,
    ) {
        source = expanded;
    }

    let parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Diagnostics(
            parse_out.errors.into_iter().map(|e| e.into()).collect(),
        ));
    }
    let mut interner = parse_out.interner;

    // Phase 1: scan declarations to build symbol table
    let decl_output = glyim_parse::declarations::parse_declarations(&source);
    let decl_table =
        glyim_hir::decl_table::DeclTable::from_declarations(&decl_output.ast, &mut interner);

    // Phase 2: full lowering with pre-resolved symbols
    let hir = glyim_hir::lower_with_declarations(&parse_out.ast, &mut interner, &decl_table);
    let mut typeck = TypeChecker::new(interner);
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::Diagnostics(
            errs.into_iter().map(|e| e.into()).collect(),
        ));
    }
    Ok(())
}
#[tracing::instrument(name = "print_ir", skip_all)]
pub fn print_ir(input: &Path) -> Result<(), PipelineError> {
    let source = format!("{}\n{}", PRELUDE, fs::read_to_string(input)?);
    let ir = compile_to_ir(&source).map_err(PipelineError::Codegen)?;
    println!("{ir}");
    Ok(())
}
pub fn init(name: &str) -> Result<PathBuf, PipelineError> {
    let dir = PathBuf::from(name);
    if dir.exists() {
        return Err(PipelineError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("'{name}' already exists"),
        )));
    }
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir)?;
    fs::write(
        dir.join("glyim.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
    )?;
    fs::write(
        src_dir.join("main.g"),
        "main = () => {\n  println(\"Hello from Glyim!\")\n}\n",
    )?;
    Ok(dir)
}
#[tracing::instrument(name = "run_with_mode", skip_all)]
pub fn run_with_mode(
    input: &Path,
    mode: BuildMode,
    target: Option<&str>,
    force_no_std: Option<bool>,
) -> Result<i32, PipelineError> {
    let (source, _) = load_source_with_prelude(input)?;
    let config = PipelineConfig {
        mode,
        target: target.map(|s| s.to_string()),
        force_no_std,
        jit_mode: true,
        ..Default::default()
    };
    let compiled = compile_source_to_hir(source, input, &config)?;
    execute_jit(&compiled, mode, target)
}
pub fn build_with_mode(
    input: &Path,
    output: Option<&Path>,
    mode: BuildMode,
    target: Option<&str>,
    force_no_std: Option<bool>,
    coverage: bool,
    library: bool,
) -> Result<PathBuf, PipelineError> {
    let (source, is_no_std) = load_source_with_prelude_opt(input, force_no_std)?;
    let config = PipelineConfig {
        mode,
        target: target.map(|s| s.to_string()),
        force_no_std: Some(is_no_std),
        coverage_mode: if coverage {
            CoverageMode::Function
        } else {
            CoverageMode::Off
        },
        ..Default::default()
    };
    let compiled = compile_source_to_hir(source, input, &config)?;
    let output_path = output.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let stem = input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        PathBuf::from(stem)
    });
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    let context = Context::create();
    let cov_mode = config.coverage_mode;
    let mut builder = CodegenBuilder::new(
        &context,
        compiled.interner.clone(),
        compiled.merged_types.clone(),
    )
    .with_coverage_mode(cov_mode);
    if library {
        builder = builder.with_library_mode();
    }
    let mut codegen = builder.build()?;
    if let Some(t) = &config.target {
        crate::cross::validate_target(t).map_err(PipelineError::Codegen)?;
        crate::cross::ensure_sysroot(t).map_err(PipelineError::MissingSysroot)?;
        codegen = codegen.with_target(t);
    }
    if compiled.is_no_std {
        codegen = codegen.with_no_std();
    }
    codegen
        .generate(&compiled.mono_hir)
        .map_err(PipelineError::Codegen)?;
    debug_ir(&codegen);
    codegen
        .write_object_file_with_opt(&obj_path, mode.opt_level())
        .map_err(PipelineError::Codegen)?;
    let cov_lib = if coverage {
        find_coverage_rt_lib()
    } else {
        None
    };
    link_object_with_coverage(
        &obj_path,
        &output_path,
        mode == BuildMode::Release,
        cov_lib.as_deref(),
    )?;
    Ok(output_path)
}
pub fn find_package_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if current.join("glyim.toml").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Execute a function via JIT with crash protection (Unix only).
/// Execute a function via JIT with crash protection (Unix only).
/// On SIGSEGV/SIGBUS, returns an error instead of aborting the process.
/// On SIGSEGV/SIGBUS, returns an error instead of aborting the process.
#[cfg(unix)]
pub fn run_jit_safe(source: &str) -> Result<i32, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static CRASHED: AtomicBool = AtomicBool::new(false);
    unsafe extern "C" fn handler(_sig: libc::c_int) {
        CRASHED.store(true, Ordering::SeqCst);
    }
    unsafe {
        let old = libc::signal(libc::SIGSEGV, handler as *const () as libc::sighandler_t);
        let _ = libc::signal(libc::SIGBUS, handler as *const () as libc::sighandler_t);
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::run_jit(source)));
        libc::signal(libc::SIGSEGV, old);
        if CRASHED.load(Ordering::SeqCst) {
            return Err("JIT execution crashed (SIGSEGV)".to_string());
        }
        result
            .map_err(|e: Box<dyn std::any::Any + Send>| format!("JIT panic: {:?}", e))
            .and_then(|r: Result<i32, crate::pipeline::PipelineError>| {
                r.map_err(|e| format!("JIT error: {:?}", e))
            })
    }
}

#[cfg(not(unix))]
#[cfg(not(unix))]
#[cfg(test)]
mod root_tests;

pub fn build_package(
    package_dir: &Path,
    output: Option<&Path>,
    mode: BuildMode,
    target: Option<&str>,
) -> Result<PathBuf, PipelineError> {
    let manifest_path = package_dir.join("glyim.toml");
    let toml_str = fs::read_to_string(&manifest_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PipelineError::Manifest(crate::manifest::ManifestError::FileNotFound(manifest_path))
        } else {
            PipelineError::Io(e)
        }
    })?;
    let full_manifest =
        glyim_pkg::manifest::parse_manifest(&toml_str, "glyim.toml").map_err(|e| {
            PipelineError::Manifest(crate::manifest::ManifestError::Parse(e.to_string()))
        })?;
    let main_path = package_dir.join("src").join("main.g");
    if !main_path.exists() {
        return Err(PipelineError::Manifest(
            crate::manifest::ManifestError::MissingField("src/main.g"),
        ));
    }
    let force_no_std = Some(is_manifest_no_std(&full_manifest));
    build_with_mode(&main_path, output, mode, target, force_no_std, false, false)
}
pub fn run_package(
    package_dir: &Path,
    mode: BuildMode,
    target: Option<&str>,
) -> Result<i32, PipelineError> {
    let manifest_path = package_dir.join("glyim.toml");
    let toml_str = fs::read_to_string(&manifest_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PipelineError::Manifest(crate::manifest::ManifestError::FileNotFound(manifest_path))
        } else {
            PipelineError::Io(e)
        }
    })?;
    let full_manifest =
        glyim_pkg::manifest::parse_manifest(&toml_str, "glyim.toml").map_err(|e| {
            PipelineError::Manifest(crate::manifest::ManifestError::Parse(e.to_string()))
        })?;
    let main_path = package_dir.join("src").join("main.g");
    if !main_path.exists() {
        return Err(PipelineError::Manifest(
            crate::manifest::ManifestError::MissingField("src/main.g"),
        ));
    }
    let force_no_std = Some(is_manifest_no_std(&full_manifest));
    run_with_mode(&main_path, mode, target, force_no_std)
}

fn find_coverage_rt_lib() -> Option<std::path::PathBuf> {
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf());
    if let Some(workspace_root) = workspace_root {
        let target_dir = workspace_root.join("target");
        for profile in &["debug", "release"] {
            let path = target_dir.join(profile).join("libglyim_coverage_rt.a");
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

fn link_object(obj_path: &Path, output_path: &Path, use_lto: bool) -> Result<(), PipelineError> {
    link_object_with_coverage(obj_path, output_path, use_lto, None)
}

fn link_object_with_coverage(
    obj_path: &Path,
    output_path: &Path,
    use_lto: bool,
    coverage_lib: Option<&std::path::Path>,
) -> Result<(), PipelineError> {
    if let Some(lib) = coverage_lib {
        // Use cc to link, force‑load the coverage archive so its symbols are always included
        let linker = if which("cc") {
            "cc"
        } else if which("gcc") {
            "gcc"
        } else {
            return Err(PipelineError::Link(
                "no C compiler found (tried 'cc' and 'gcc')".into(),
            ));
        };
        let mut args: Vec<std::ffi::OsString> = vec![
            "-o".into(),
            output_path.as_os_str().into(),
            obj_path.as_os_str().into(),
            "-lc".into(),
            "-no-pie".into(),
        ];
        // Force load the entire coverage runtime archive on macOS / with Apple clang
        if cfg!(target_vendor = "apple") {
            args.push("-Wl,-force_load".into());
        } else {
            args.push("-Wl,--whole-archive".into());
        }
        args.push(lib.as_os_str().into());
        if !cfg!(target_vendor = "apple") {
            args.push("-Wl,--no-whole-archive".into());
        }
        if use_lto {
            args.push("-flto=thin".into());
        }
        let output = Command::new(linker)
            .args(&args)
            .output()
            .map_err(|e| PipelineError::Link(format!("failed to invoke '{linker}': {e}")))?;
        if !output.status.success() {
            return Err(PipelineError::Link(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
    } else {
        let linker = if which("cc") {
            "cc"
        } else if which("gcc") {
            "gcc"
        } else {
            return Err(PipelineError::Link(
                "no C compiler found (tried 'cc' and 'gcc')".into(),
            ));
        };
        let mut args: Vec<std::ffi::OsString> = vec![
            "-o".into(),
            output_path.as_os_str().into(),
            obj_path.as_os_str().into(),
            "-lc".into(),
            "-no-pie".into(),
        ];
        if use_lto {
            args.push("-flto=thin".into());
        }
        let output = Command::new(linker)
            .args(&args)
            .output()
            .map_err(|e| PipelineError::Link(format!("failed to invoke '{linker}': {e}")))?;
        if !output.status.success() {
            return Err(PipelineError::Link(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
    }
    Ok(())
}
fn which(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
/// Generate HTML documentation for the given source file.
#[allow(dead_code)]
pub fn run_doctests(input: &Path) -> Result<usize, PipelineError> {
    let source = std::fs::read_to_string(input).map_err(PipelineError::Io)?;
    let parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Diagnostics(
            parse_out.errors.into_iter().map(|e| e.into()).collect(),
        ));
    }
    let mut interner = parse_out.interner;
    let mut hir = glyim_hir::lower(&parse_out.ast, &mut interner);
    glyim_hir::attach_doc_comments(&mut hir, &glyim_lex::tokenize(&source));

    let mut blocks: Vec<String> = Vec::new();
    for item in &hir.items {
        let doc = match item {
            glyim_hir::HirItem::Fn(f) => &f.doc,
            glyim_hir::HirItem::Struct(s) => &s.doc,
            glyim_hir::HirItem::Enum(e) => &e.doc,
            glyim_hir::HirItem::Impl(i) => {
                for method in &i.methods {
                    if let Some(ref doc) = method.doc {
                        for (_, code) in glyim_doc::extract_code_blocks(doc) {
                            blocks.push(code);
                        }
                    }
                }
                continue;
            }
            _ => continue,
        };
        if let Some(doc) = doc.as_ref() {
            for (_, code) in glyim_doc::extract_code_blocks(doc) {
                blocks.push(code);
            }
        }
    }

    if blocks.is_empty() {
        tracing::debug!("No doc-test blocks found.");
        return Ok(0);
    }

    let mut failed = 0;
    for (i, block) in blocks.iter().enumerate() {
        tracing::debug!("running {} doc-test(s)", blocks.len());
        tracing::debug!("doc-test block {} ... ", i + 1);
        // Run as a simple expression via JIT (wrap in main = () => { ... })
        let wrapped = format!("main = () => {{ {} }}", block);
        match run_jit(&wrapped) {
            Ok(exit_code) => {
                if exit_code == 0 {
                    tracing::debug!("ok");
                } else {
                    tracing::debug!("FAILED (exit code {})", exit_code);
                    failed += 1;
                }
            }
            Err(e) => {
                tracing::debug!("FAILED: {}", e);
                failed += 1;
            }
        }
    }
    Ok(failed)
}

pub fn generate_doc(input: &Path, output_dir: Option<&Path>) -> Result<(), PipelineError> {
    let source = std::fs::read_to_string(input).map_err(PipelineError::Io)?;
    let parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Diagnostics(
            parse_out.errors.into_iter().map(|e| e.into()).collect(),
        ));
    }
    let mut interner = parse_out.interner;
    let mut hir = glyim_hir::lower(&parse_out.ast, &mut interner);
    glyim_hir::attach_doc_comments(&mut hir, &glyim_lex::tokenize(&source));
    let html = glyim_doc::generate_html(&hir, &interner);
    let out_dir = output_dir.unwrap_or_else(|| Path::new("doc"));
    std::fs::create_dir_all(out_dir).map_err(PipelineError::Io)?;
    std::fs::write(out_dir.join("index.html"), html).map_err(PipelineError::Io)?;
    Ok(())
}

/// Print generated LLVM IR to stderr when GLYIM_DEBUG_IR is set.
fn debug_ir(_codegen: &glyim_codegen_llvm::Codegen) {
    if std::env::var("GLYIM_DEBUG_IR").is_ok() {}
}

#[cfg(test)]
mod no_std_tests;

/// Execute a single test function via JIT, returning the test exit code.
pub fn run_jit_test(source: &str, test_name: &str) -> Result<i32, PipelineError> {
    let parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Diagnostics(
            parse_out.errors.into_iter().map(|e| e.into()).collect(),
        ));
    }
    let mut interner = parse_out.interner;
    let decl_output = glyim_parse::declarations::parse_declarations(source);
    let decl_table =
        glyim_hir::decl_table::DeclTable::from_declarations(&decl_output.ast, &mut interner);
    let mut hir = glyim_hir::lower_with_declarations(&parse_out.ast, &mut interner, &decl_table);
    let mut typeck = glyim_typeck::TypeChecker::new(interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::Diagnostics(
            errs.into_iter().map(|e| e.into()).collect(),
        ));
    }
    glyim_hir::desugar_method_calls(&mut hir, &typeck.expr_types, &mut interner);
    let expr_types = typeck.expr_types.clone();
    let call_type_args = std::mem::take(&mut typeck.call_type_args);
    let (merged_types, mono_hir) =
        merge_mono_types(&hir, &mut interner, &expr_types, &call_type_args);
    let context = inkwell::context::Context::create();
    let mut cg = CodegenBuilder::new(&context, interner, merged_types).build()?;
    cg = cg.with_jit_mode();
    cg.generate(&mono_hir).map_err(PipelineError::Codegen)?;
    debug_ir(&cg);
    let engine = cg
        .get_module()
        .create_jit_execution_engine(inkwell::OptimizationLevel::None)
        .map_err(|e| PipelineError::Codegen(format!("JIT: {e}")))?;
    runtime_shims::map_runtime_shims_for_jit(
        &engine,
        cg.get_module(),
        *CUSTOM_ASSERT_FN.lock().unwrap(),
        *CUSTOM_ABORT_FN.lock().unwrap(),
    );
    unsafe {
        let test_fn = engine
            .get_function::<unsafe extern "C" fn() -> i32>(test_name)
            .map_err(|e| {
                PipelineError::Codegen(format!("JIT test function '{}': {e}", test_name))
            })?;
        Ok(test_fn.call())
    }
}

/// Execute source code via JIT compilation and return its exit code.
///
/// # Stability
/// *Stable.*
/// Execute source code via JIT compilation and return its exit code.
///
/// # Stability
/// *Stable.*
pub fn run_jit_with_coverage(
    source: &str,
    cov_path: &std::path::Path,
) -> Result<i32, PipelineError> {
    let config = PipelineConfig {
        mode: BuildMode::Debug,
        coverage_mode: glyim_codegen_llvm::codegen::CoverageMode::Function,
        coverage_output: Some(cov_path.to_path_buf()),
        ..Default::default()
    };
    run_jit_with_config(source, &config)
}

fn run_jit_with_config(source: &str, config: &PipelineConfig) -> Result<i32, PipelineError> {
    // existing run_jit code but using provided config
    ProfileCollector::enter_stage(StageName::Parse);
    ProfileCollector::enter_stage(StageName::Codegen);
    let parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Diagnostics(
            parse_out.errors.into_iter().map(|e| e.into()).collect(),
        ));
    }
    let mut interner = parse_out.interner;
    let decl_output = glyim_parse::declarations::parse_declarations(source);
    let decl_table =
        glyim_hir::decl_table::DeclTable::from_declarations(&decl_output.ast, &mut interner);
    let mut hir = glyim_hir::lower_with_declarations(&parse_out.ast, &mut interner, &decl_table);
    let mut typeck = TypeChecker::new(interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::Diagnostics(
            errs.into_iter().map(|e| e.into()).collect(),
        ));
    }
    glyim_hir::desugar_method_calls(&mut hir, &typeck.expr_types, &mut interner);
    let expr_types = typeck.expr_types.clone();
    let call_type_args = std::mem::take(&mut typeck.call_type_args);
    let (merged_types, mono_hir) =
        merge_mono_types(&hir, &mut interner, &expr_types, &call_type_args);
    let context = Context::create();
    let mut cg = CodegenBuilder::new(&context, interner, merged_types)
        .with_coverage_mode(config.coverage_mode)
        .build()?;
    cg = cg.with_jit_mode();
    if let Some(ref path) = config.coverage_output {
        unsafe {
            std::env::set_var("GLYIM_COV_FILE", path.to_string_lossy().as_ref());
        }
    }
    cg.generate(&mono_hir).map_err(PipelineError::Codegen)?;
    debug_ir(&cg);
    let engine = cg
        .get_module()
        .create_jit_execution_engine(OptimizationLevel::None)
        .map_err(|e| PipelineError::Codegen(format!("JIT: {e}")))?;
    runtime_shims::map_runtime_shims_for_jit(
        &engine,
        cg.get_module(),
        *CUSTOM_ASSERT_FN.lock().unwrap(),
        *CUSTOM_ABORT_FN.lock().unwrap(),
    );
    unsafe {
        let main_fn = engine
            .get_function::<unsafe extern "C" fn() -> i32>("main")
            .map_err(|e| PipelineError::Codegen(format!("JIT main: {e}")))?;
        let exit_code = main_fn.call();
        ProfileCollector::exit_stage(StageName::Codegen, 1, 0, 0);
        ProfileCollector::exit_stage(StageName::Parse, 1, 0, 0);
        Ok(exit_code)
    }
}

pub fn run_jit(source: &str) -> Result<i32, PipelineError> {
    let config = PipelineConfig::default();
    run_jit_with_config(source, &config)
}
