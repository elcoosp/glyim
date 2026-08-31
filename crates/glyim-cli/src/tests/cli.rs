#[cfg(test)]
mod tests {
    use crate::CliArgs;
    use crate::run_with_args;
    use std::path::PathBuf;

    #[test]
    fn test_unknown_emit_returns_error() {
        let args = CliArgs {
            input: PathBuf::from("test.g"),
            output: None,
            emit: "unknown".to_string(),
            opt_level: 0,
            target: None,
            backend: "llvm".to_string(),
            linker: None,
            link_flags: None,
            lto: "off".to_string(),
            codegen_units: None,
            proc_macro_deps: None,
        error_format: "human".to_string(),
        };
        let result = run_with_args(args);
        assert!(result.is_err());
        let errs = result.unwrap_err();
        assert!(!errs.is_empty());
        assert!(errs[0].message.contains("invalid value for --emit"));
        assert!(
            errs[0]
                .message
                .contains("expected one of: obj, exec, mir, llvm-ir")
        );
    }

    #[test]
    fn test_emit_mir_writes_file() {
        use std::io::Write;
        let mut src = tempfile::NamedTempFile::new().expect("create temp file");
        write!(src, "fn main() {{ }}").expect("write source");
        let src_path = src.path().to_path_buf();
        let output = src_path.with_extension("mir");
        let _ = std::fs::remove_file(&output);

        let args = CliArgs {
            input: src_path.clone(),
            output: None, // Use default output path
            emit: "mir".to_string(),
            opt_level: 0,
            target: None,
            backend: "llvm".to_string(),
            linker: None,
            link_flags: None,
            lto: "off".to_string(),
            codegen_units: None,
            proc_macro_deps: None,
        error_format: "human".to_string(),
        };
        let result = run_with_args(args);
        assert!(result.is_ok(), "emit_mir failed: {:?}", result.err());
        assert!(output.exists(), "MIR file was not written to {:?}", output);
        std::fs::remove_file(&output).ok();
    }

    #[test]
    fn test_emit_llvm_ir_writes_file() {
        use std::io::Write;
        let mut src = tempfile::NamedTempFile::new().expect("create temp file");
        write!(src, "fn main() {{ }}").expect("write source");
        let src_path = src.path().to_path_buf();
        let output = src_path.with_extension("ll");
        let _ = std::fs::remove_file(&output);

        let args = CliArgs {
            input: src_path.clone(),
            output: None, // Use default output path
            emit: "llvm-ir".to_string(),
            opt_level: 0,
            target: None,
            backend: "llvm".to_string(),
            linker: None,
            link_flags: None,
            lto: "off".to_string(),
            codegen_units: None,
            proc_macro_deps: None,
        error_format: "human".to_string(),
        };
        let result = run_with_args(args);
        assert!(result.is_ok(), "emit_llvm_ir failed: {:?}", result.err());
        assert!(
            output.exists(),
            "LLVM IR file was not written to {:?}",
            output
        );
        std::fs::remove_file(&output).ok();
    }
}
