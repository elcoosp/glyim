use glyim_compiler::pipeline::{self, BuildMode};
use glyim_orchestrator::orchestrator::{PackageGraphOrchestrator, OrchestratorConfig};
use std::path::PathBuf;

pub fn cmd_run(input: PathBuf, target: Option<String>, release: bool, live: bool, incremental: bool, remote_cache: Option<String>) -> i32 {
    let mode = if release {
        BuildMode::Release
    } else {
        BuildMode::Debug
    };

    // If workspace, use orchestrator run (which calls run_jit on main package)
    if let Some(root) = glyim_compiler::pipeline::find_package_root(&input)
        && root.join("glyim.toml").exists() {
            let config = OrchestratorConfig {
                mode,
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
            match orch.run() {
                Ok(exit_code) => exit_code,
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            };
            return 0;
        }

    if incremental {
        let source = match std::fs::read_to_string(&input) {
            Ok(s) => s,
            Err(e) => { eprintln!("error reading {}: {}", input.display(), e); return 1; }
        };
        return match pipeline::run_live(&source) {
            Ok(code) => code,
            Err(e) => { eprintln!("error: {e}"); 1 }
        };
    }
    if live {
        let source = match std::fs::read_to_string(&input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error reading {}: {}", input.display(), e);
                return 1;
            }
        };
        return match pipeline::run_live(&source) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        };
    }
    let result = if input.is_dir() {
        pipeline::run_package(&input, mode, target.as_deref())
    } else {
        pipeline::run_with_mode(&input, mode, target.as_deref(), None)
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
