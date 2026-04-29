use glyim_codegen_llvm::{compile_to_ir, Codegen};
use glyim_interner::Interner;
use glyim_typeck::TypeChecker;
use glyim_typeck::TypeError;
use inkwell::context::Context;
use std::path::{Path, PathBuf};
use std::{fs, process::Command};

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

pub fn build(input: &Path, output: Option<&Path>) -> Result<PathBuf, PipelineError> {
    let source = format!("{}\n{}", PRELUDE, fs::read_to_string(input)?);
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
    let context = Context::create();
    let mut codegen = Codegen::new(&context, interner, typeck.expr_types.clone());
    codegen.generate(&hir).map_err(PipelineError::Codegen)?;
    codegen
        .write_object_file(&obj_path)
        .map_err(PipelineError::Codegen)?;
    link_object(&obj_path, &output)?;
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

pub fn run(input: &Path) -> Result<i32, PipelineError> {
    let source = format!("{}\n{}", PRELUDE, fs::read_to_string(input)?);
    let mut parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        let rendered = glyim_diag::render_diagnostics(
            &source,
            &input.to_string_lossy(),
            &parse_out
                .errors
                .iter()
                .map(|e| {
                    let span = match e {
                        glyim_parse::ParseError::Expected { span, .. } => {
                            Some(glyim_diag::Span::new(span.0, span.1))
                        }
                        glyim_parse::ParseError::UnexpectedEof { .. } => None,
                        glyim_parse::ParseError::ExpectedExpr { span, .. } => {
                            Some(glyim_diag::Span::new(span.0, span.1))
                        }
                        glyim_parse::ParseError::Message { span, .. } => {
                            Some(glyim_diag::Span::new(span.0, span.1))
                        }
                    };
                    glyim_diag::Diagnostic::error(e.to_string()).with_span_opt(span)
                })
                .collect::<Vec<_>>(),
        );
        eprintln!("{rendered}");
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    let mut typeck = TypeChecker::new(parse_out.interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }
    let context = Context::create();
    let mut codegen = Codegen::new(&context, parse_out.interner, vec![]);
    codegen.generate(&hir).map_err(PipelineError::Codegen)?;
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    codegen
        .write_object_file(&obj_path)
        .map_err(PipelineError::Codegen)?;
    let exe_path = tmp_dir.path().join("glyim_out");
    link_object(&obj_path, &exe_path)?;
    let status = Command::new(&exe_path)
        .status()
        .map_err(PipelineError::Run)?;
    Ok(status.code().unwrap_or(1))
}

pub fn check(input: &Path) -> Result<(), PipelineError> {
    let source = format!("{}\n{}", PRELUDE, fs::read_to_string(input)?);
    let mut parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        let rendered = glyim_diag::render_diagnostics(
            &source,
            &input.to_string_lossy(),
            &parse_out
                .errors
                .iter()
                .map(|e| {
                    let span = match e {
                        glyim_parse::ParseError::Expected { span, .. } => {
                            Some(glyim_diag::Span::new(span.0, span.1))
                        }
                        glyim_parse::ParseError::UnexpectedEof { .. } => None,
                        glyim_parse::ParseError::ExpectedExpr { span, .. } => {
                            Some(glyim_diag::Span::new(span.0, span.1))
                        }
                        glyim_parse::ParseError::Message { span, .. } => {
                            Some(glyim_diag::Span::new(span.0, span.1))
                        }
                    };
                    glyim_diag::Diagnostic::error(e.to_string()).with_span_opt(span)
                })
                .collect::<Vec<_>>(),
        );
        eprintln!("{rendered}");
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    let mut typeck = TypeChecker::new(parse_out.interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }
    Ok(())
}

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

pub fn run_with_mode(input: &Path, _mode: BuildMode) -> Result<i32, PipelineError> {
    let source = format!("{}
{}", PRELUDE, fs::read_to_string(input)?);
    let mut parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        let rendered = glyim_diag::render_diagnostics(
            &source,
            &input.to_string_lossy(),
            &parse_out
                .errors
                .iter()
                .map(|e| {
                    let span = match e {
                        glyim_parse::ParseError::Expected { span, .. } => {
                            Some(glyim_diag::Span::new(span.0, span.1))
                        }
                        glyim_parse::ParseError::UnexpectedEof { .. } => None,
                        glyim_parse::ParseError::ExpectedExpr { span, .. } => {
                            Some(glyim_diag::Span::new(span.0, span.1))
                        }
                        glyim_parse::ParseError::Message { span, .. } => {
                            Some(glyim_diag::Span::new(span.0, span.1))
                        }
                    };
                    glyim_diag::Diagnostic::error(e.to_string()).with_span_opt(span)
                })
                .collect::<Vec<_>>(),
        );
        eprintln!("{rendered}");
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    let mut typeck = TypeChecker::new(parse_out.interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }
    let context = Context::create();
    let mut codegen = Codegen::new(&context, parse_out.interner, typeck.expr_types.clone());
    codegen.generate(&hir).map_err(PipelineError::Codegen)?;
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    codegen
        .write_object_file(&obj_path)
        .map_err(PipelineError::Codegen)?;
    let exe_path = tmp_dir.path().join("glyim_out");
    link_object(&obj_path, &exe_path)?;
    let status = Command::new(&exe_path)
        .status()
        .map_err(PipelineError::Run)?;
    Ok(status.code().unwrap_or(1))
}

pub fn build_with_mode(input: &Path, output: Option<&Path>, _mode: BuildMode) -> Result<PathBuf, PipelineError> {
    let source = format!("{}
{}", PRELUDE, fs::read_to_string(input)?);
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
    let context = Context::create();
    let mut codegen = Codegen::new(&context, interner, typeck.expr_types.clone());
    codegen.generate(&hir).map_err(PipelineError::Codegen)?;
    codegen
        .write_object_file(&obj_path)
        .map_err(PipelineError::Codegen)?;
    link_object(&obj_path, &output)?;
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
        return Err(PipelineError::Manifest(crate::manifest::ManifestError::MissingField(
            "src/main.g",
        )));
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
    let _manifest =
        crate::manifest::parse_manifest(&toml_str).map_err(PipelineError::Manifest)?;

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


pub fn run_tests(input: &Path) -> Result<crate::test_runner::TestRunSummary, PipelineError> {
    let source = format!("{}
{}", PRELUDE, fs::read_to_string(input)?);
    let mut parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        let rendered = glyim_diag::render_diagnostics(&source, &input.to_string_lossy(),
            &parse_out.errors.iter().map(|e| {
                let span = match e {
                        glyim_parse::ParseError::Expected { span, .. } => Some(glyim_diag::Span::new(span.0, span.1)),
                        glyim_parse::ParseError::UnexpectedEof { .. } => None,
                        glyim_parse::ParseError::ExpectedExpr { span, .. } => Some(glyim_diag::Span::new(span.0, span.1)),
                        glyim_parse::ParseError::Message { span, .. } => Some(glyim_diag::Span::new(span.0, span.1)),
                    };
                glyim_diag::Diagnostic::error(e.to_string()).with_span_opt(span)
            }).collect::<Vec<_>>());
        eprintln!("{rendered}");
        return Err(PipelineError::Parse(parse_out.errors));
    }

    let test_fns = crate::test_runner::collect_test_functions(&parse_out.ast, &parse_out.interner, None, false);
    if test_fns.is_empty() {
        return Err(PipelineError::Codegen("no #[test] functions found".into()));
    }

    let active_names: Vec<String> = test_fns.iter()
        .filter(|t| !t.ignored)
        .map(|t| t.name.clone())
        .collect();
    let ignored_names: Vec<String> = test_fns.iter()
        .filter(|t| t.ignored)
        .map(|t| t.name.clone())
        .collect();

    if active_names.is_empty() {
        let results: Vec<_> = ignored_names.iter()
            .map(|n| (n.clone(), crate::test_runner::TestResult::Ignored))
            .collect();
        return Ok(crate::test_runner::TestRunSummary { results });
    }

    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    let mut typeck = TypeChecker::new(parse_out.interner.clone());
    if let Err(errs) = typeck.check(&hir) {
        return Err(PipelineError::TypeCheck(errs));
    }

    let context = Context::create();
    let mut codegen = Codegen::new(&context, parse_out.interner, typeck.expr_types.clone());
    codegen.generate_for_tests(&hir, &active_names).map_err(PipelineError::Codegen)?;
    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");
    codegen.write_object_file(&obj_path).map_err(PipelineError::Codegen)?;
    let exe_path = tmp_dir.path().join("glyim_test_out");
    link_object(&obj_path, &exe_path)?;

    let output = Command::new(&exe_path).output().map_err(PipelineError::Run)?;
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

pub fn run_tests_package(package_dir: &Path) -> Result<crate::test_runner::TestRunSummary, PipelineError> {
    let main_path = package_dir.join("src").join("main.g");
    if !main_path.exists() {
        return Err(PipelineError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, format!("{} not found", main_path.display()))));
    }
    run_tests(&main_path)
}

fn compile_to_hir_and_ir(
    source: &str,
) -> Result<(glyim_hir::Hir, String, Interner), PipelineError> {
    let mut parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let hir = glyim_hir::lower(&parse_out.ast, &mut parse_out.interner);
    let ir = compile_to_ir(source).map_err(PipelineError::Codegen)?;
    Ok((hir, ir, parse_out.interner))
}

fn link_object(obj_path: &Path, output_path: &Path) -> Result<(), PipelineError> {
    let linker = if which("cc") {
        "cc"
    } else if which("gcc") {
        "gcc"
    } else {
        return Err(PipelineError::Link(
            "no C compiler found (tried 'cc' and 'gcc')".into(),
        ));
    };
    let output = Command::new(linker)
        .arg("-o")
        .arg(output_path)
        .arg(obj_path)
        .output()
        .map_err(|e| PipelineError::Link(format!("failed to invoke '{linker}': {e}")))?;
    if !output.status.success() {
        return Err(PipelineError::Link(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
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
