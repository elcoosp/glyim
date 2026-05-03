use std::path::PathBuf;
use std::process::Command;

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

/// Cross‑compilation toolchain package names for common platforms.
fn toolchain_package_name(triple: &str) -> Option<&str> {
    match triple {
        "aarch64-unknown-linux-gnu" => Some("gcc-aarch64-linux-gnu"),
        "x86_64-unknown-linux-gnu" => Some("gcc-x86-64-linux-gnu"),
        // macOS targets don't need external packages
        _ => None,
    }
}

/// Try to install the cross‑compilation toolchain automatically.
/// Returns Ok(()) if installation succeeded (or wasn't needed), Err if it failed.
pub fn install_sysroot(triple: &str) -> Result<(), String> {
    // macOS Darwin targets use the built‑in SDK
    if triple.contains("darwin") {
        // Verify the macOS SDK is available via xcrun
        let output = Command::new("xcrun")
            .args(["--sdk", "macosx", "--show-sdk-path"])
            .output()
            .map_err(|e| format!("failed to run xcrun: {e}"))?;

        if output.status.success() {
            let sdk_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            eprintln!("Using macOS SDK: {}", sdk_path);
            return Ok(());
        }
        return Err("macOS SDK not available; install Xcode or Command Line Tools".to_string());
    }

    // Linux targets: try to install the cross‑compilation toolchain
    let pkg = toolchain_package_name(triple)
        .ok_or_else(|| format!("no toolchain package defined for target '{triple}'"))?;

    eprintln!("Attempting to install cross‑compilation toolchain...");

    // Try apt-get first (Debian/Ubuntu)
    let apt_result = Command::new("sudo")
        .args(["apt-get", "install", "-y", pkg])
        .status();

    if apt_result.is_ok_and(|s| s.success()) {
        eprintln!("Installed {pkg} via apt-get");
        return Ok(());
    }

    // Try dnf (Fedora)
    let dnf_result = Command::new("sudo")
        .args(["dnf", "install", "-y", pkg])
        .status();

    if dnf_result.is_ok_and(|s| s.success()) {
        eprintln!("Installed {pkg} via dnf");
        return Ok(());
    }

    // Try pacman (Arch)
    let pacman_result = Command::new("sudo")
        .args(["pacman", "-S", "--noconfirm", pkg])
        .status();

    if pacman_result.is_ok_and(|s| s.success()) {
        eprintln!("Installed {pkg} via pacman");
        return Ok(());
    }

    Err(format!(
        "Could not install '{pkg}' automatically.\n\
         Install it manually:\n\
           Ubuntu/Debian: sudo apt-get install {pkg}\n\
           Fedora:        sudo dnf install {pkg}\n\
           Arch:          sudo pacman -S {pkg}\n\
         Then re‑run your build."
    ))
}

/// Check whether the sysroot for the given target triple exists.
/// If not, attempt to install it automatically.
pub fn ensure_sysroot(triple: &str) -> Result<(), String> {
    // For macOS targets, check if the SDK is available
    if triple.contains("darwin") {
        let output = Command::new("xcrun")
            .args(["--sdk", "macosx", "--show-sdk-path"])
            .output();

        match output {
            Ok(out) if out.status.success() => return Ok(()),
            _ => {}
        }
    }

    // For Linux targets, check if the cross‑compiler is available
    if let Some(pkg) = toolchain_package_name(triple) {
        // Try the common cross‑compiler patterns
        let patterns = [
            format!("{triple}-gcc"),
            format!("{pkg}"),
        ];
        for pat in &patterns {
            let status = Command::new("which").arg(pat).status();
            if status.is_ok_and(|s| s.success()) {
                return Ok(());
            }
        }
    }

    // Not found — try to install
    let install_result = install_sysroot(triple);
    match install_result {
        Ok(()) => Ok(()),
        Err(e) => Err(format!(
            "Sysroot for '{triple}' not available.\n{e}\n\
             Please install the cross‑compilation toolchain and re‑run."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_target_is_valid() {
        let host = super::host_target();
        assert!(super::validate_target(&host).is_ok());
    }

    #[test]
    fn all_supported_targets_are_valid() {
        for target in super::SUPPORTED_TARGETS {
            assert!(
                super::validate_target(target).is_ok(),
                "target {} should be valid",
                target
            );
        }
    }

    #[test]
    fn invalid_target_fails_validation() {
        assert!(super::validate_target("mips-unknown-none").is_err());
        assert!(super::validate_target("bogus-triple").is_err());
    }

    #[test]
    fn missing_sysroot_returns_error() {
        // A bogus target won't have a toolchain package, so ensure_sysroot should fail
        assert!(super::ensure_sysroot("bogus-triple").is_err());
    }

    #[test]
    fn toolchain_package_for_aarch64() {
        assert_eq!(
            toolchain_package_name("aarch64-unknown-linux-gnu"),
            Some("gcc-aarch64-linux-gnu")
        );
    }

    #[test]
    fn toolchain_package_for_darwin_is_none() {
        // Darwin doesn't need external packages
        assert_eq!(toolchain_package_name("aarch64-apple-darwin"), None);
    }

    #[test]
    fn host_target_validates() {
        let host = host_target();
        assert!(validate_target(&host).is_ok());
    }
}
