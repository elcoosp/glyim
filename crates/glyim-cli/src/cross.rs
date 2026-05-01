use std::path::PathBuf;

/// Supported target triples for cross-compilation.
pub const SUPPORTED_TARGETS: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

/// Get the default target triple for the host.
pub fn host_target() -> String {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin".to_string()
        } else {
            "x86_64-apple-darwin".to_string()
        }
    } else {
        if cfg!(target_arch = "aarch64") {
            "aarch64-unknown-linux-gnu".to_string()
        } else {
            "x86_64-unknown-linux-gnu".to_string()
        }
    }
}

/// Validate a target triple.
pub fn validate_target(triple: &str) -> Result<(), String> {
    if SUPPORTED_TARGETS.contains(&triple) {
        Ok(())
    } else {
        Err(format!(
            "unsupported target '{}'. Supported: {:?}",
            triple, SUPPORTED_TARGETS
        ))
    }
}

/// Get the sysroot directory for a target triple.
pub fn sysroot_dir(triple: &str) -> PathBuf {
    dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from(".glyim"))
        .join("sysroots")
        .join(triple)
}
