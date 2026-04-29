use glyim_codegen_llvm::{compile_to_ir, Codegen};
use glyim_interner::Interner;
use inkwell::context::Context;
use std::path::{Path, PathBuf};
use std::{fs, process::Command};

#[derive(Debug)]
pub enum PipelineError {
    Io(std::io::Error),
    Parse(Vec<glyim_parse::ParseError>),
    Codegen(String),
    Link(String),
    Run(std::io::Error),
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
            Self::Link(msg) => write!(f, "linker error: {msg}"),
            Self::Run(e) => write!(f, "execution error: {e}"),
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
    let source = fs::read_to_string(input)?;
    let (hir, _ir, interner) = compile_to_hir_and_ir(&source)?;
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
    let mut codegen = Codegen::new(&context, interner);
    codegen.generate(&hir).map_err(PipelineError::Codegen)?;
    codegen
        .write_object_file(&obj_path)
        .map_err(PipelineError::Codegen)?;
    link_object(&obj_path, &output)?;
    Ok(output)
}

pub fn run(input: &Path) -> Result<i32, PipelineError> {
    let source = fs::read_to_string(input)?;
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
    let context = Context::create();
    let mut codegen = Codegen::new(&context, parse_out.interner);
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
    let source = fs::read_to_string(input)?;
    let parse_out = glyim_parse::parse(&source);
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
    Ok(())
}

pub fn print_ir(input: &Path) -> Result<(), PipelineError> {
    let source = fs::read_to_string(input)?;
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
