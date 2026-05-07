use glyim_compiler::pipeline::{self, BuildMode};
use glyim_orchestrator::orchestrator::{PackageGraphOrchestrator, OrchestratorConfig};
use std::path::PathBuf;

pub fn cmd_build(
    input: PathBuf,
    output: Option<PathBuf>,
    target: Option<String>,
    release: bool,
    bare: bool,
    incremental: bool,
    remote_cache: Option<String>,
    profile: bool,
) -> i32 {
    let mode = if release {
        BuildMode::Release
    } else {
        BuildMode::Debug
    };
    if profile {
        glyim_profiler::ProfileCollector::enable();
    }
    if coverage {
        // TODO: pass coverage mode into pipeline config
        eprintln!("Coverage mode enabled (function-level instrumentation)");
    }
    if incremental {
        eprintln!("Using incremental compilation pipeline...");
        return match pipeline::build_incremental(&input, output.as_deref(), mode, target.as_deref()) {
            Ok((_path, report)) => {
                eprintln!("Incremental build: {:?} ({:.1}ms)",
                    if report.was_full_rebuild { "full rebuild" } else { "incremental" },
                    report.total_elapsed.as_secs_f64() * 1000.0);
                0
            }
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        };
    }

    // If a workspace is detected, use the orchestrator
    if let Some(root) = glyim_compiler::pipeline::find_package_root(&input)
        && root.join("glyim.toml").exists() {
            let config = OrchestratorConfig {
                mode: if release { BuildMode::Release } else { BuildMode::Debug },
                target: target.clone(),
                remote_cache_url: remote_cache.clone(),
                ..Default::default()
            };
            let mut orch = match PackageGraphOrchestrator::new(&root, config) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("error: orchestrator init: {e}");
                    return 1;
                }
            };
            match orch.build() {
                Ok(_bin_path) => {
                    eprintln!("Workspace build complete.");
            if profile { print_profile(); }
                    let report = orch.report();
                    eprintln!("Compiled packages: {:?}", report.packages_compiled);
                    if !report.packages_failed.is_empty() {
                        eprintln!("Failed packages: {:?}", report.packages_failed);
                        return 1;
                    }
                    return 0;
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            }
        }

    // Fallback: single file compilation
    let result = if bare || input.is_file() {
        pipeline::build_with_mode(&input, output.as_deref(), mode, target.as_deref(), None)
    } else {
        pipeline::build_package(&input, output.as_deref(), mode, target.as_deref())
    };
    match result {
        Ok(path) => {
            eprintln!("Built: {}", path.display());
            if profile { print_profile(); }
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}


fn print_profile() {
    let p = glyim_profiler::ProfileCollector::finish();
    eprintln!("\n=== Compilation Profile ===");
    eprintln!("Total duration: {:.1}ms", p.total_duration.as_secs_f64() * 1000.0);
    let mut stages: Vec<_> = p.stages.iter().collect();
    stages.sort_by_key(|(_, s)| s.duration);
    for (name, stage) in stages.iter().rev() {
        if stage.duration.as_nanos() > 0 {
            eprintln!("  {:?}: {:.3}ms ({} items, {} hits, {} misses)",
                name, stage.duration.as_secs_f64() * 1000.0,
                stage.items_processed, stage.cache_hits, stage.cache_misses);
        }
    }
    if p.incremental.total_items > 0 {
        eprintln!("Incremental: {} green / {} red, hit rate {:.1}%",
            p.incremental.green_items, p.incremental.red_items,
            p.incremental.cache_hit_ratio * 100.0);
    }
}
