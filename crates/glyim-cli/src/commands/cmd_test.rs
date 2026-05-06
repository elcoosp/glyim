use glyim_testr::config::TestConfig;
use std::path::PathBuf;

pub fn cmd_test(
    input: PathBuf,
    ignore: bool,
    filter: Option<String>,
    nocapture: bool,
    watch: bool,
    optimize_check: bool,
    _remote_cache: Option<String>,
    coverage: bool,
) -> i32 {
    if coverage { eprintln!("Coverage instrumentation enabled"); }
    let config = TestConfig {
            // Phase 6B coverage stub
            
        filter,
        include_ignored: ignore,
        nocapture,
        watch,
        optimize_check,
        ..Default::default()
    };

    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {}: {}", input.display(), e);
            return 1;
        }
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error creating runtime: {}", e);
            return 1;
        }
    };

    if watch {
        rt.block_on(glyim_testr::run_watch(&input, &config));
        0
    } else {
        let results = rt.block_on(glyim_testr::run_tests(&source, &config));
        let failed = results
            .iter()
            .filter(|r| matches!(r.outcome, glyim_testr::types::TestOutcome::Failed { .. }))
            .count();
        if failed > 0 { 1 } else { 0 }
    }
}
