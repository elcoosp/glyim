use std::process::Command;
use std::fs;
use tempfile::tempdir;

/// Run FileCheck on the generated LLVM IR from the source.
/// Returns list of FileCheck failures (empty if passes).
pub fn check_optimization(source: &str) -> Result<Vec<String>, String> {
    let ir = glyim_codegen_llvm::compile_to_ir(source).map_err(|e| e.to_string())?;
    let tmp = tempdir().map_err(|e| e.to_string())?;
    let ir_path = tmp.path().join("test.ll");
    let src_path = tmp.path().join("test.g");
    fs::write(&ir_path, &ir).map_err(|e| e.to_string())?;
    fs::write(&src_path, source).map_err(|e| e.to_string())?;

    let output = Command::new("FileCheck")
        .arg("--input-file")
        .arg(&ir_path)
        .arg(&src_path)
        .output()
        .map_err(|e| format!("failed to run FileCheck: {e}"))?;

    if output.status.success() {
        Ok(vec![])
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok(stderr.lines().map(|l| l.to_string()).collect())
    }
}
