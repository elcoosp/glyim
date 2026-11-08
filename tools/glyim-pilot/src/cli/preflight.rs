use crate::config::PilotConfig;
use std::sync::Arc;

/// `run_preflight`.
pub async fn run_preflight(config: &Arc<PilotConfig>) {
    println!("Running preflight checks...");
    match tokio::process::Command::new("git")
        .args(["--version"])
        .output()
        .await
    {
        Ok(o) if o.status.success() => {
            println!("✅ git: {}", String::from_utf8_lossy(&o.stdout).trim());
        }
        _ => println!("❌ git: not found"),
    }
    match tokio::process::Command::new("cargo")
        .args(["--version"])
        .output()
        .await
    {
        Ok(o) if o.status.success() => {
            println!("✅ cargo: {}", String::from_utf8_lossy(&o.stdout).trim());
        }
        _ => println!("❌ cargo: not found"),
    }
    println!("Providers: {} configured", config.providers.len());
    println!("Gate level: {}", config.gates.level);
    println!(
        "Default branch: {} ({})",
        config.execution.default_branch, config.execution.branch_version
    );
}
