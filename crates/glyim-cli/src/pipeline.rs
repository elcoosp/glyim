use glyim_codegen_llvm::{compile_to_ir, Codegen};
use glyim_interner::Interner;
use glyim_pkg::cas_client::CasClient;
use glyim_typeck::TypeChecker;
use glyim_typeck::TypeError;
use inkwell::context::Context;
use std::path::{Path, PathBuf};
use std::{fs, process::Command};
use tracing::{info, info_span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[derive(Debug, Clone)]
pub struct BuildConfig {
    pub mode: BuildMode,
    pub target: Option<String>,
}

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
pub enum PipelineError {
    Io(std::io::Error),
    Parse(Vec<glyim_parse::ParseError>),
    Codegen(String),
    TypeCheck(Vec<TypeError>),
    Link(String),
    Run(std::io::Error),
    Manifest(crate::manifest::ManifestError),
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
        }
    }
}

impl std::error::Error for PipelineError {}
impl From<std::io::Error> for PipelineError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Detect whether the source contains a bare `no_std` declaration.
///
/// A `no_std` declaration is a line where `no_std` appears as a standalone
/// identifier — not part of a larger word, not inside a string literal.
///
/// Known limitation: does not filter out `no_std` inside comments.
/// This is acceptable for v0.5.1 — the full parser handles comments correctly.
fn detect_no_std(source: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed == "no_std" {
            return true;
        }
    }
    false
}

/// Load source with prelude prepended, detect no_std.
/// Returns (full_source, is_no_std_flag).
fn load_source_with_prelude(input: &Path) -> Result<(String, bool), PipelineError> {
    let source = format!("{}\n{}", PRELUDE, fs::read_to_string(input)?);
    let is_no_std = detect_no_std(&source);
    Ok((source, is_no_std))
}

#[tracing::instrument(name = "build", skip_all)]
pub fn build(input: &Path, output: Option<&Path>) -> Result<PathBuf, PipelineError> {
    let (source, is_no_std) = load_source_with_prelude(input)?;
    let (hir, _ir, interner) = compile_to_hir_and_ir(&source)?;
    let mut typeck = TypeChecker::new(interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }
    let output = output.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let stem = input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        PathBuf::from(stem)
    });
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    let _codegen_span = info_span!("phase", name = "codegen").entered();
    let context = Context::create();
    info!("starting codegen");
    let mut codegen =
        Codegen::with_line_tables(&context, interner, typeck.expr_types.clone(), source)
            .map_err(PipelineError::Codegen)?;
    if is_no_std {
        codegen = codegen.with_no_std();
    }
    codegen.generate(&hir).map_err(PipelineError::Codegen)?;
    info!("codegen complete");
    codegen
        .write_object_file(&obj_path)
        .map_err(PipelineError::Codegen)?;
    link_object(&obj_path, &output, false)?;
    Ok(output)
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
";

#[tracing::instrument(name = "run", skip_all)]
pub fn run(input: &Path) -> Result<i32, PipelineError> {
    let (source, is_no_std) = load_source_with_prelude(input)?;
    let _parse_span = info_span!("phase", name = "parse").entered();
    let mut parse_out = glyim_parse::parse(&source);
    info!("parsed {} items", parse_out.ast.items.len());
    if !parse_out.errors.is_empty() {
        for e in &parse_out.errors {
            eprintln!("{:?}", glyim_diag::Report::new(e.clone()));
        }
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let _lower_span = info_span!("phase", name = "lower").entered();
    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    info!("lowered to HIR");
    let _typeck_span = info_span!("phase", name = "typeck").entered();
    let mut typeck = TypeChecker::new(parse_out.interner.clone());
    info!("typeck registered items");
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }
    let _codegen_span = info_span!("phase", name = "codegen").entered();
    let context = Context::create();
    info!("starting codegen");
    let mut codegen =
        Codegen::with_line_tables(&context, parse_out.interner, vec![], source.clone())
            .map_err(PipelineError::Codegen)?;
    if is_no_std {
        codegen = codegen.with_no_std();
    }
    codegen.generate(&hir).map_err(PipelineError::Codegen)?;
    info!("codegen complete");
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    codegen
        .write_object_file(&obj_path)
        .map_err(PipelineError::Codegen)?;
    let exe_path = tmp_dir.path().join("glyim_out");
    eprintln!("----- LLVM IR -----");
    eprintln!("{}", codegen.ir_string());
    eprintln!("----- LLVM IR -----");
    eprintln!("{}", codegen.ir_string());
    link_object(&obj_path, &exe_path, false)?;
    let status = Command::new(&exe_path)
        .status()
        .map_err(PipelineError::Run)?;
    Ok(status.code().unwrap_or(1))
}

#[tracing::instrument(name = "check", skip_all)]
pub fn check(input: &Path) -> Result<(), PipelineError> {
    let source = format!("{}\n{}", PRELUDE, fs::read_to_string(input)?);
    let _parse_span = info_span!("phase", name = "parse").entered();
    let mut parse_out = glyim_parse::parse(&source);
    info!("parsed {} items", parse_out.ast.items.len());
    if !parse_out.errors.is_empty() {
        for e in &parse_out.errors {
            eprintln!("{:?}", glyim_diag::Report::new(e.clone()));
        }
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let _lower_span = info_span!("phase", name = "lower").entered();
    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    info!("lowered to HIR");
    let _typeck_span = info_span!("phase", name = "typeck").entered();
    let mut typeck = TypeChecker::new(parse_out.interner.clone());
    info!("typeck registered items");
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
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
pub fn run_with_mode(input: &Path, mode: BuildMode) -> Result<i32, PipelineError> {
    let (source, is_no_std) = load_source_with_prelude(input)?;
    let _parse_span = info_span!("phase", name = "parse").entered();
    let mut parse_out = glyim_parse::parse(&source);
    info!("parsed {} items", parse_out.ast.items.len());
    if !parse_out.errors.is_empty() {
        for e in &parse_out.errors {
            eprintln!("{:?}", glyim_diag::Report::new(e.clone()));
        }
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let _lower_span = info_span!("phase", name = "lower").entered();
    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    info!("lowered to HIR");
    let _typeck_span = info_span!("phase", name = "typeck").entered();
    let mut typeck = TypeChecker::new(parse_out.interner.clone());
    info!("typeck registered items");
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }
    let _codegen_span = info_span!("phase", name = "codegen").entered();
    let context = Context::create();
    info!("starting codegen");
    let mut codegen = match mode {
        BuildMode::Debug => Codegen::with_debug(
            &context,
            parse_out.interner,
            typeck.expr_types.clone(),
            source.clone(),
        )
        .map_err(PipelineError::Codegen)?,
        BuildMode::Release => Codegen::new(&context, parse_out.interner, typeck.expr_types.clone()),
    };
    if is_no_std {
        codegen = codegen.with_no_std();
    }
    codegen.generate(&hir).map_err(PipelineError::Codegen)?;
    info!("codegen complete");
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    codegen
        .write_object_file_with_opt(&obj_path, mode.opt_level())
        .map_err(PipelineError::Codegen)?;
    let exe_path = tmp_dir.path().join("glyim_out");
    eprintln!("----- LLVM IR -----");
    eprintln!("{}", codegen.ir_string());
    eprintln!("----- LLVM IR -----");
    eprintln!("{}", codegen.ir_string());
    link_object(&obj_path, &exe_path, mode == BuildMode::Release)?;
    let status = Command::new(&exe_path)
        .status()
        .map_err(PipelineError::Run)?;
    Ok(status.code().unwrap_or(1))
}

pub fn build_with_mode(
    input: &Path,
    output: Option<&Path>,
    mode: BuildMode,
) -> Result<PathBuf, PipelineError> {
    let (source, is_no_std) = load_source_with_prelude(input)?;
    let (hir, _ir, interner) = compile_to_hir_and_ir(&source)?;
    let mut typeck = TypeChecker::new(interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }
    let output = output.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let stem = input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        PathBuf::from(stem)
    });
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    let _codegen_span = info_span!("phase", name = "codegen").entered();
    let context = Context::create();
    info!("starting codegen");
    let mut codegen = match mode {
        BuildMode::Debug => Codegen::with_debug(
            &context,
            interner,
            typeck.expr_types.clone(),
            source.clone(),
        )
        .map_err(PipelineError::Codegen)?,
        BuildMode::Release => Codegen::new(&context, interner, typeck.expr_types.clone()),
    };
    if is_no_std {
        codegen = codegen.with_no_std();
    }
    codegen.generate(&hir).map_err(PipelineError::Codegen)?;
    info!("codegen complete");
    codegen
        .write_object_file_with_opt(&obj_path, mode.opt_level())
        .map_err(PipelineError::Codegen)?;
    link_object(&obj_path, &output, mode == BuildMode::Release)?;
    Ok(output)
}

/// Walk from `start` upward looking for `glyim.toml`.
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

#[cfg(test)]
mod root_tests {
    use super::*;

    #[test]
    fn find_package_root_in_current_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("glyim.toml"), "[package]\nname = \"x\"\n").unwrap();
        let result = find_package_root(dir.path());
        assert_eq!(result, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn find_package_root_in_parent_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("glyim.toml"), "[package]\nname = \"x\"\n").unwrap();
        let child = dir.path().join("src");
        std::fs::create_dir_all(&child).unwrap();
        let result = find_package_root(&child);
        assert_eq!(result, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn find_package_root_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = find_package_root(dir.path());
        assert_eq!(result, None);
    }

    #[test]
    fn find_package_root_stops_at_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("glyim.toml"), "[package]\nname = \"x\"\n").unwrap();
        let file_path = dir.path().join("src/main.g");
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(&file_path, "main = () => 42").unwrap();
        let result = find_package_root(&file_path);
        assert_eq!(result, Some(dir.path().to_path_buf()));
    }
}

pub fn build_package(
    package_dir: &Path,
    output: Option<&Path>,
    mode: BuildMode,
) -> Result<PathBuf, PipelineError> {
    let manifest_path = package_dir.join("glyim.toml");
    let toml_str = fs::read_to_string(&manifest_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PipelineError::Manifest(crate::manifest::ManifestError::FileNotFound(manifest_path))
        } else {
            PipelineError::Io(e)
        }
    })?;
    let _manifest = crate::manifest::parse_manifest(&toml_str).map_err(PipelineError::Manifest)?;

    let main_path = package_dir.join("src").join("main.g");
    if !main_path.exists() {
        return Err(PipelineError::Manifest(
            crate::manifest::ManifestError::MissingField("src/main.g"),
        ));
    }

    build_with_mode(&main_path, output, mode)
}

pub fn run_package(package_dir: &Path, mode: BuildMode) -> Result<i32, PipelineError> {
    let manifest_path = package_dir.join("glyim.toml");
    let toml_str = fs::read_to_string(&manifest_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PipelineError::Manifest(crate::manifest::ManifestError::FileNotFound(manifest_path))
        } else {
            PipelineError::Io(e)
        }
    })?;
    let _manifest = crate::manifest::parse_manifest(&toml_str).map_err(PipelineError::Manifest)?;

    let main_path = package_dir.join("src").join("main.g");
    if !main_path.exists() {
        return Err(PipelineError::Manifest(
            crate::manifest::ManifestError::MissingField("src/main.g"),
        ));
    }

    run_with_mode(&main_path, mode)
}

fn parse_test_output(stdout: &str) -> Vec<(String, crate::test_runner::TestResult)> {
    let mut results = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("test ") {
            if let Some(name_end) = rest.find(" ... ") {
                let name = &rest[..name_end];
                let status = &rest[name_end + 5..];
                if status == "ok" {
                    results.push((name.to_string(), crate::test_runner::TestResult::Passed));
                } else if status == "FAILED" {
                    results.push((name.to_string(), crate::test_runner::TestResult::Failed));
                }
            }
        }
    }
    results
}

pub fn run_tests(
    input: &Path,
    filter_name: Option<&str>,
    include_ignored: bool,
) -> Result<crate::test_runner::TestRunSummary, PipelineError> {
    let (source, is_no_std) = load_source_with_prelude(input)?;
    let _parse_span = info_span!("phase", name = "parse").entered();
    let mut parse_out = glyim_parse::parse(&source);
    info!("parsed {} items", parse_out.ast.items.len());
    if !parse_out.errors.is_empty() {
        for e in &parse_out.errors {
            eprintln!("{:?}", glyim_diag::Report::new(e.clone()));
        }
        return Err(PipelineError::Parse(parse_out.errors));
    }

    let test_fns = crate::test_runner::collect_test_functions(
        &parse_out.ast,
        &parse_out.interner,
        filter_name,
        true,
    );
    // Collect should_panic test names from AST attributes
    let should_panic: std::collections::HashSet<String> = test_fns
        .iter()
        .filter(|t| {
            parse_out.ast.items.iter().any(|item| {
                if let glyim_parse::Item::FnDef { attrs, name, .. } = item {
                    parse_out.interner.resolve(*name) == t.name
                        && attrs.iter().any(|attr| {
                            attr.name == "test"
                                && attr.args.iter().any(|arg| arg.key == "should_panic")
                        })
                } else {
                    false
                }
            })
        })
        .map(|t| t.name.clone())
        .collect();

    if test_fns.is_empty() {
        return Err(PipelineError::Codegen("no #[test] functions found".into()));
    }

    let (active_names, ignored_names) = if include_ignored {
        let all: Vec<String> = test_fns.iter().map(|t| t.name.clone()).collect();
        (all, Vec::new())
    } else {
        let active: Vec<String> = test_fns
            .iter()
            .filter(|t| !t.ignored)
            .map(|t| t.name.clone())
            .collect();
        let ignored: Vec<String> = test_fns
            .iter()
            .filter(|t| t.ignored)
            .map(|t| t.name.clone())
            .collect();
        (active, ignored)
    };

    if active_names.is_empty() {
        let results: Vec<_> = ignored_names
            .iter()
            .map(|n| (n.clone(), crate::test_runner::TestResult::Ignored))
            .collect();
        return Ok(crate::test_runner::TestRunSummary { results });
    }
    let _lower_span = info_span!("phase", name = "lower").entered();
    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    info!("lowered to HIR");
    let _typeck_span = info_span!("phase", name = "typeck").entered();
    let mut typeck = TypeChecker::new(parse_out.interner.clone());
    info!("typeck registered items");
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }

    let _codegen_span = info_span!("phase", name = "codegen").entered();
    let context = Context::create();
    info!("starting codegen");
    let mut codegen = Codegen::new(&context, parse_out.interner, typeck.expr_types.clone());
    if is_no_std {
        codegen = codegen.with_no_std();
    }
    codegen
        .generate_for_tests(&hir, &active_names, &should_panic)
        .map_err(PipelineError::Codegen)?;
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    codegen
        .write_object_file(&obj_path)
        .map_err(PipelineError::Codegen)?;
    let exe_path = tmp_dir.path().join("glyim_test_out");
    link_object(&obj_path, &exe_path, false)?;

    let output = Command::new(&exe_path)
        .output()
        .map_err(PipelineError::Run)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = parse_test_output(&stdout);

    for name in &ignored_names {
        results.push((name.clone(), crate::test_runner::TestResult::Ignored));
    }

    if !output.status.success() && output.status.code().is_none() {
        for (_, ref mut result) in results.iter_mut() {
            if *result == crate::test_runner::TestResult::Passed {
                *result = crate::test_runner::TestResult::Failed;
            }
        }
    }

    Ok(crate::test_runner::TestRunSummary { results })
}

pub fn run_tests_package(
    package_dir: &Path,
    filter_name: Option<&str>,
    include_ignored: bool,
) -> Result<crate::test_runner::TestRunSummary, PipelineError> {
    let main_path = package_dir.join("src").join("main.g");
    if !main_path.exists() {
        return Err(PipelineError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} not found", main_path.display()),
        )));
    }
    run_tests(&main_path, filter_name, include_ignored)
}

fn compile_to_hir_and_ir(
    source: &str,
) -> Result<(glyim_hir::Hir, String, Interner), PipelineError> {
    let mut parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let _lower_span = info_span!("phase", name = "lower").entered();
    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    info!("lowered to HIR");
    let ir = compile_to_ir(source).map_err(PipelineError::Codegen)?;
    Ok((hir, ir, parse_out.interner))
}

fn link_object(obj_path: &Path, output_path: &Path, use_lto: bool) -> Result<(), PipelineError> {
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
    Ok(())
}

/// Compute a source hash for build caching.
#[allow(dead_code)]
fn compute_source_hash(source: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    // Mix in glyim version to invalidate cache on compiler changes
    hasher.update(env!("CARGO_PKG_VERSION"));
    hex::encode(hasher.finalize())
}

/// Build using a content-addressable cache.
/// If the source hash matches a cached object, reuse it; otherwise compile and store.
#[allow(dead_code)]
fn build_with_cache(input: &Path, output: Option<&Path>) -> Result<PathBuf, PipelineError> {
    let source = format!("{}\n{}", PRELUDE, fs::read_to_string(input)?);
    let hash = compute_source_hash(&source);
    let cache_dir = dirs_next::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".glyim/cache"))
        .join("glyim-objects");
    let cas = CasClient::new(&cache_dir).map_err(PipelineError::Io)?;

    let output = output.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let stem = input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        PathBuf::from(stem)
    });

    let hash_content = hash.parse::<glyim_macro_vfs::ContentHash>().unwrap();
    if let Some(cached_obj) = cas.retrieve(hash_content) {
        let tmp_dir = tempfile::tempdir()?;
        let obj_path = tmp_dir.path().join("cached.o");
        fs::write(&obj_path, &cached_obj)?;
        eprintln!("[cache] Reusing cached object for {}", input.display());
        link_object(&obj_path, &output, false)?;
        return Ok(output);
    }

    // Compile as usual
    let (hir, _ir, interner) = compile_to_hir_and_ir(&source)?;
    let mut typeck = TypeChecker::new(interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    let _codegen_span = info_span!("phase", name = "codegen").entered();
    let context = Context::create();
    info!("starting codegen");
    let mut codegen = Codegen::new(&context, interner, typeck.expr_types.clone());
    codegen.generate(&hir).map_err(PipelineError::Codegen)?;
    info!("codegen complete");
    codegen
        .write_object_file(&obj_path)
        .map_err(PipelineError::Codegen)?;

    // Store the object in cache
    let obj_bytes = fs::read(&obj_path)?;
    cas.store(&obj_bytes);
    eprintln!("[cache] Stored object for {}", input.display());

    link_object(&obj_path, &output, false)?;
    Ok(output)
}

fn which(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod no_std_tests {
    use super::*;

    #[test]

    fn detect_no_std_simple() {
        assert!(detect_no_std("no_std\nfn main() { 0 }"));
    }

    #[test]

    fn detect_no_std_at_start() {
        assert!(detect_no_std("no_std\nfn main() { 0 }"));
    }

    #[test]

    fn detect_no_std_false_when_absent() {
        assert!(!detect_no_std("fn main() { 0 }"));
    }

    #[test]

    fn detect_no_std_false_in_string() {
        assert!(!detect_no_std(r#"fn main() { "no_std" }"#));
    }

    #[test]

    fn detect_no_std_false_as_part_of_ident() {
        assert!(!detect_no_std("fn no_std_helper() { 0 }"));
    }

    #[test]

    fn detect_no_std_false_as_field_name() {
        assert!(!detect_no_std("struct S { no_std: bool }"));
    }

    #[test]

    fn detect_no_std_with_trailing_whitespace() {
        assert!(detect_no_std("no_std   \nfn main() { 0 }"));
    }

    #[test]

    fn detect_no_std_false_empty() {
        assert!(!detect_no_std(""));
    }

    #[test]

    fn detect_no_std_false_only_whitespace() {
        assert!(!detect_no_std("  \n  \n"));
    }

    #[test]

    fn detect_no_std_after_other_code() {
        assert!(detect_no_std("fn foo() { 0 }\nno_std\nfn bar() { 0 }"));
    }

    #[test]

    fn detect_no_std_known_limitation_comment() {
        // Comments on their own line are correctly excluded
        // because the trimmed line is "// no_std", not "no_std".
        assert!(!detect_no_std(
            "// no_std
fn main() { 0 }"
        ));
    }
}

#[cfg(feature = "jit")]
pub fn run_jit(source: &str) -> Result<i32, PipelineError> {
    use glyim_codegen_llvm::Codegen;
    use glyim_interner::Interner;
    use glyim_typeck::TypeChecker;
    use inkwell::context::Context;
    use inkwell::OptimizationLevel;

    let mut parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
    let mut typeck = TypeChecker::new(interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }

    let context = Context::create();
    let mut cg = Codegen::new(&context, interner, typeck.expr_types);
    cg.generate(&hir).map_err(PipelineError::Codegen)?;

    let engine = cg
        .get_module()
        .create_jit_execution_engine(OptimizationLevel::None)
        .map_err(|e| PipelineError::Codegen(format!("JIT: {e}")))?;

    unsafe {
        let main_fn = engine
            .get_function::<unsafe extern "C" fn() -> i32>("main")
            .map_err(|e| PipelineError::Codegen(format!("JIT main: {e}")))?;
        Ok(main_fn.call())
    }
}
