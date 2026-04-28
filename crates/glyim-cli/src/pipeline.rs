use std::path::{Path, PathBuf};
use std::process::Command;
use glyim_codegen_llvm::{compile_to_ir, Codegen};
use glyim_interner::Interner;
use inkwell::context::Context;

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
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::Parse(errs) => {
                writeln!(f, "{} parse error(s):", errs.len())?;
                for e in errs { writeln!(f, "  - {}", e)?; }
                Ok(())
            }
            Self::Codegen(msg) => write!(f, "codegen error: {}", msg),
            Self::Link(msg) => write!(f, "linker error: {}", msg),
            Self::Run(e) => write!(f, "execution error: {}", e),
        }
    }
}

impl std::error::Error for PipelineError {}

impl From<std::io::Error> for PipelineError {
    fn from(e: std::io::Error) -> Self { Self::Io(e) }
}

fn compile_to_hir_and_ir(source: &str) -> Result<(glyim_hir::Hir, String, Interner), PipelineError> {
    let parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Parse(parse_out.errors));
    }
    let hir = glyim_hir::lower(&parse_out.ast, &parse_out.interner);
    let ir = compile_to_ir(source).map_err(PipelineError::Codegen)?;
    Ok((hir, ir, parse_out.interner))
}

pub fn build(input: &Path, output: Option<&Path>) -> Result<PathBuf, PipelineError> {
    let source = std::fs::read_to_string(input)?;
    let (hir, _ir, interner) = compile_to_hir_and_ir(&source)?;

    let output = output.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let stem = input.file_stem().unwrap_or_default().to_string_lossy().to_string();
        PathBuf::from(stem)
    });

    let tmp_dir = tempfile::tempdir()?;
    let obj_path = tmp_dir.path().join("output.o");

    let context = Context::create();
    let mut codegen = Codegen::new(&context, interner);
    codegen.generate(&hir).map_err(PipelineError::Codegen)?;
    codegen.write_object_file(&obj_path).map_err(PipelineError::Codegen)?;

    link_object(&obj_path, &output)?;
    Ok(output)
}

pub fn run(input: &Path) -> Result<i32, PipelineError> {
    let tmp_dir = tempfile::tempdir()?;
    let exe_path = tmp_dir.path().join("glyim_out");
    build(input, Some(&exe_path))?;
    let status = Command::new(&exe_path).status().map_err(PipelineError::Run)?;
    Ok(status.code().unwrap_or(1))
}

pub fn check(input: &Path) -> Result<(), PipelineError> {
    let source = std::fs::read_to_string(input)?;
    let parse_out = glyim_parse::parse(&source);
    if !parse_out.errors.is_empty() {
        return Err(PipelineError::Parse(parse_out.errors));
    }
    Ok(())
}

pub fn print_ir(input: &Path) -> Result<(), PipelineError> {
    let source = std::fs::read_to_string(input)?;
    let ir = compile_to_ir(&source).map_err(PipelineError::Codegen)?;
    println!("{ir}");
    Ok(())
}

fn link_object(obj_path: &Path, output_path: &Path) -> Result<(), PipelineError> {
    let linker = if which_cc("cc") { "cc" } else if which_cc("gcc") { "gcc" } else {
        return Err(PipelineError::Link("no C compiler found (tried 'cc' and 'gcc')".into()));
    };
    let output = std::process::Command::new(linker)
        .arg("-o").arg(output_path)
        .arg(obj_path)
        .output()
        .map_err(|e| PipelineError::Link(format!("failed to invoke '{}': {}", linker, e)))?;
    if !output.status.success() {
        return Err(PipelineError::Link(String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(())
}

fn which_cc(name: &str) -> bool {
    std::process::Command::new(name).arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status().map_or(false, |s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn compile_main_ir() {
        let (hir, ir, _) = compile_to_hir_and_ir("main = () => 42").unwrap();
        assert_eq!(hir.fns.len(), 1);
        assert!(ir.contains("@main"));
    }
    #[test] fn parse_error_propagates() {
        let res = compile_to_hir_and_ir("main = +");
        assert!(matches!(res.unwrap_err(), PipelineError::Parse(_)));
    }
    #[test] fn no_main_error() {
        let res = compile_to_hir_and_ir("fn other() { 1 }");
        assert!(matches!(res.unwrap_err(), PipelineError::Codegen(_)));
    }
}
