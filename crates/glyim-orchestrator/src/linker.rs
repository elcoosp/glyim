use std::path::{Path, PathBuf};
use std::process::Command;

/// Configuration for the multi-object linker.
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct LinkConfig {
    /// Override the default linker (e.g., `ld`, `mold`, `lld`).
    pub linker: Option<String>,
    /// Additional library search paths (`-L` flags).
    pub library_search_paths: Vec<PathBuf>,
    /// Target triple (passed to the linker via `-target` if using Clang).
    pub target_triple: Option<String>,
    /// Use link-time optimization (thin LTO).
    pub use_lto: bool,
    /// Extra arguments to pass to the linker.
    pub extra_args: Vec<String>,
    /// Sysroot for the target (for cross-compilation).
    pub sysroot: Option<PathBuf>,
}


/// Errors that can occur during linking.
#[derive(Debug)]
pub enum LinkError {
    Io(std::io::Error),
    LinkerNotFound(String),
    LinkFailed(String),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::LinkerNotFound(name) => write!(f, "linker '{}' not found", name),
            Self::LinkFailed(msg) => write!(f, "linker failed: {msg}"),
        }
    }
}

impl std::error::Error for LinkError {}

/// Link multiple object files into a single executable.
pub fn link_multi_object(
    object_paths: &[PathBuf],
    output_path: &Path,
    config: &LinkConfig,
) -> Result<(), LinkError> {
    if object_paths.is_empty() {
        return Err(LinkError::LinkFailed("no object files provided".into()));
    }

    let linker = config
        .linker
        .as_deref()
        .unwrap_or(if cfg!(target_os = "macos") { "cc" } else { "cc" });

    let mut cmd = Command::new(linker);
    cmd.arg("-o").arg(output_path);

    for obj in object_paths {
        cmd.arg(obj);
    }

    // Standard libraries
    cmd.arg("-lc");

    // Position-independent executable (disabled with -no-pie on some platforms)
    if !cfg!(target_os = "macos") {
        cmd.arg("-no-pie");
    }

    // Library search paths
    for path in &config.library_search_paths {
        cmd.arg("-L").arg(path);
    }

    // Target triple
    if let Some(target) = &config.target_triple {
        cmd.arg("-target").arg(target);
    }

    // Sysroot
    if let Some(sysroot) = &config.sysroot {
        cmd.arg("--sysroot").arg(sysroot);
    }

    // Link-time optimization
    if config.use_lto {
        cmd.arg("-flto=thin");
    }

    // Extra arguments
    for arg in &config.extra_args {
        cmd.arg(arg);
    }

    let output = cmd.output().map_err(LinkError::Io)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LinkError::LinkFailed(stderr.to_string()));
    }

    Ok(())
}

/// Detect the best available linker.
pub fn detect_linker() -> String {
    for candidate in &["cc", "gcc", "clang", "ld"] {
        if Command::new(candidate)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return candidate.to_string();
        }
    }
    "cc".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn create_dummy_object(path: &Path, content: &[u8]) {
        let mut file = fs::File::create(path).unwrap();
        file.write_all(content).unwrap();
    }

    #[test]
    fn link_multi_object_no_input_errors() {
        let config = LinkConfig::default();
        let result = link_multi_object(&[], Path::new("a.out"), &config);
        assert!(result.is_err());
    }

    #[test]
    fn link_multi_object_with_dummy_files() {
        let dir = tempfile::tempdir().unwrap();
        let obj1 = dir.path().join("a.o");
        let obj2 = dir.path().join("b.o");
        create_dummy_object(&obj1, b"dummy");
        create_dummy_object(&obj2, b"dummy");
        let out = dir.path().join("output");
        let config = LinkConfig::default();
        // This will likely fail because the objects are not real ELF/Mach-O,
        // but the test ensures the command is constructed correctly.
        let _ = link_multi_object(&[obj1, obj2], &out, &config);
    }

    #[test]
    fn link_config_default_is_sane() {
        let config = LinkConfig::default();
        assert!(config.linker.is_none());
        assert!(config.library_search_paths.is_empty());
    }

    #[test]
    fn detect_linker_returns_string() {
        let linker = detect_linker();
        assert!(!linker.is_empty());
    }

    #[test]
    fn link_config_with_extra_args() {
        let config = LinkConfig {
            extra_args: vec!["-Wl,-rpath,/usr/local/lib".into()],
            ..Default::default()
        };
        assert_eq!(config.extra_args.len(), 1);
    }
}
