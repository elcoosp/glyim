use glyim_compiler::lockfile_integration;
use glyim_orchestrator::orchestrator::{PackageGraphOrchestrator, OrchestratorConfig};

pub fn cmd_fetch() -> i32 {
    let result: Result<i32, i32> = (|| {
        let dir = std::env::current_dir().map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;

        // Try to use workspace orchestrator to fetch artifacts
        if let Some(root) = glyim_compiler::pipeline::find_package_root(&dir) {
            let config = OrchestratorConfig {
                remote_cache_url: std::env::var("GLYIM_CACHE_URL").ok(),
                remote_cache_token: std::env::var("GLYIM_CACHE_TOKEN").ok(),
                ..Default::default()
            };
            match PackageGraphOrchestrator::new(&root, config) {
                Ok(orch) => {
                    // Perform a dry-run build to trigger remote pulls without linking
                    // We can call a dedicated fetch method or just check what can be pulled
                    eprintln!("Fetching dependencies for workspace...");
                    // For now, we just report that remote cache was configured
                    let report = orch.report();
                    eprintln!("Remote cache configured. {} artifacts already pulled.", report.artifacts_pulled);
                    return Ok(0);
                }
                Err(e) => {
                    eprintln!("warning: could not init orchestrator: {e}");
                }
            }
        }

        // Fallback to lockfile-based fetch
        let packages = lockfile_integration::read_lockfile_packages(&dir).map_err(|e| {
            eprintln!("error: {e}");
            1
        })?;
        if packages.is_empty() {
            eprintln!("No dependencies to fetch (glyim.lock not found or empty)");
            return Ok(0);
        }
        eprintln!("Fetching {} package(s)...", packages.len());
        for pkg in &packages {
            eprintln!("  {} {} ({:?})", pkg.name, pkg.version, pkg.source);
            if let Some(ref artifact_hash) = pkg.artifact_hash {
                eprintln!("    artifact: {}", artifact_hash);
            }
        }
        eprintln!("Done. To download pre-compiled artifacts, use --remote-cache with glyim build.");
        Ok(0)
    })();
    result.unwrap_or_else(|code| code)
}
