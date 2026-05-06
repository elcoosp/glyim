use glyim_compiler::lockfile_integration;
use glyim_orchestrator::orchestrator::{PackageGraphOrchestrator, OrchestratorConfig};
use glyim_macro_vfs::ContentHash;
use std::path::PathBuf;

pub fn cmd_fetch() -> i32 {
    let result: Result<i32, i32> = (|| {
        let dir = std::env::current_dir().map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;

        // Try workspace orchestrator first
        if let Some(root) = glyim_compiler::pipeline::find_package_root(&dir) {
            let remote_url = std::env::var("GLYIM_CACHE_URL").ok();
            let remote_token = std::env::var("GLYIM_CACHE_TOKEN").ok();

            let config = OrchestratorConfig {
                remote_cache_url: remote_url.clone(),
                remote_cache_token: remote_token,
                force_rebuild: false,
                ..Default::default()
            };

            match PackageGraphOrchestrator::new(&root, config) {
                Ok(orch) => {
                    eprintln!("Workspace orchestrator initialized.");
                    if remote_url.is_some() {
                        eprintln!("Remote cache configured: {}", remote_url.unwrap());
                        eprintln!("Artifacts will be pulled on demand during builds.");
                    } else {
                        eprintln!("No remote cache configured.");
                        eprintln!("Set GLYIM_CACHE_URL and GLYIM_CACHE_TOKEN to enable remote caching.");
                    }
                    return Ok(0);
                }
                Err(e) => {
                    eprintln!("warning: orchestrator init failed: {e}");
                }
            }
        }

        // Fallback to lockfile-based info
        let packages = lockfile_integration::read_lockfile_packages(&dir).map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;
        if packages.is_empty() {
            eprintln!("No dependencies to fetch (glyim.lock not found or empty)");
            return Ok(0);
        }
        eprintln!("Found {} package(s) in lockfile:", packages.len());
        for pkg in &packages {
            eprintln!("  {} {} ({:?})", pkg.name, pkg.version, pkg.source);
            if let Some(ref artifact_hash) = pkg.artifact_hash {
                eprintln!("    artifact: {}", artifact_hash);
            }
        }
        eprintln!("To download pre-compiled artifacts, set GLYIM_CACHE_URL and run 'glyim build --remote-cache <URL>'.");
        Ok(0)
    })();
    result.unwrap_or_else(|code| code)
}
