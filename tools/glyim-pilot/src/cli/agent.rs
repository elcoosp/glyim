use crate::config::PilotConfig;
use crate::dispatch::provider_pool::ProviderPool;
use crate::dispatch::wave::{dispatch_wave, DispatchStrategy};
use crate::session::persistence::StatePersistence;
use clap::Subcommand;
use reqwest::Client;
use serde_json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use walkdir::WalkDir;

#[derive(Subcommand)]
/// AgentCommands.
pub enum AgentCommands {
    /// Run a single stream (e.g., S01)
    Run {
        #[arg(short, long)]
        /// Struct.
        stream: String,
        #[arg(short, long)]
        /// Struct.
        provider: Option<String>,
        #[arg(short, long, default_value_t = false)]
        /// Struct.
        wait: bool,
    },
    /// Run a wave (all streams in current wave)
    Wave {
        /// Proceed to the next wave after manual merge
        #[arg(short, long, default_value_t = false)]
        next: bool,
    },
    /// Manually mark a session's PR as merged (fallback for auto-detection)
    MarkMerged {
        #[arg(short, long)]
        /// Struct.
        stream: String,
    },
}

/// handle_agent_command.
pub async fn handle_agent_command(
    cmd: AgentCommands,
    config: &Arc<PilotConfig>,
    project_root: &Path,
) -> Result<(), anyhow::Error> {
    match cmd {
        AgentCommands::Run {
            stream,
            provider,
            wait,
        } => {
            run_stream(
                stream,
                provider,
                config.clone(),
                project_root.to_path_buf(),
                wait,
            )
            .await?;
        }
        AgentCommands::Wave { next } => {
            run_wave(next, config, project_root).await?;
        }
        AgentCommands::MarkMerged { stream } => {
            mark_merged(stream, config, project_root).await?;
        }
    }
    Ok(())
}

/// Determine the repository root directory.
/// If we are inside `tools/glyim-pilot`, the repo root is two levels up.
/// Otherwise, use the given project_root.
fn find_repo_root(project_root: &Path) -> PathBuf {
    // Try to go up two levels from project_root (assuming we are in tools/glyim-pilot)
    let candidate = project_root.join("../../");
    if candidate.join("docs/agent-kit").exists() {
        candidate.canonicalize().unwrap_or(candidate)
    } else if project_root.join("docs/agent-kit").exists() {
        project_root.to_path_buf()
    } else {
        // Fallback to project_root
        project_root.to_path_buf()
    }
}

/// Run a single stream: assemble prompts, start session, optionally wait for completion.
async fn run_stream(
    stream_id: String,
    provider_override: Option<String>,
    config: Arc<PilotConfig>,
    project_root: PathBuf,
    wait: bool,
) -> Result<(), anyhow::Error> {
    let provider = provider_override.unwrap_or_else(|| config.defaults.provider.clone());
    let (system_prompt, user_prompt) = assemble_prompts(&stream_id, &project_root).await?;

    let client = Client::new();
    let resp = client
        .post("http://127.0.0.1:8421/session/start")
        .json(&serde_json::json!({
            "provider": provider,
            "prompt": user_prompt,
            "session_id": stream_id,
            "system_prompt": system_prompt,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await?;
        anyhow::bail!("Failed to start session: {}", text);
    }
    println!("Session started for stream {}", stream_id);

    if wait {
        println!("Waiting for session to complete...");
        let status = wait_for_session(
            &stream_id,
            Duration::from_secs(5),
            Duration::from_secs(3600),
        )
        .await?;
        println!("Session finished with status: {}", status);
        if status != "committed" && status != "escalated" {
            anyhow::bail!("Session did not complete successfully (status: {})", status);
        }
    }
    Ok(())
}

/// Wait for a session to exit the "running" state.
/// Wait for a session to exit the "running" or "unknown" state.
async fn wait_for_session(
    session_id: &str,
    poll_interval: Duration,
    timeout: Duration,
) -> Result<String, anyhow::Error> {
    let client = Client::new();
    let start = std::time::Instant::now();
    let mut unknown_start = None;
    loop {
        let resp = client
            .get(format!(
                "http://127.0.0.1:8421/session/{}/status",
                session_id
            ))
            .send()
            .await?;
        if resp.status().is_success() {
            let json: serde_json::Value = resp.json().await?;
            let status = json["status"].as_str().unwrap_or("unknown");
            if status != "running" && status != "unknown" {
                return Ok(status.to_string());
            }
            if status == "unknown" {
                if unknown_start.is_none() {
                    unknown_start = Some(std::time::Instant::now());
                } else if let Some(start) = unknown_start {
                    if start.elapsed() > Duration::from_secs(600) {
                        anyhow::bail!("Session never started (status unknown for 10 minutes)");
                    }
                }
            } else {
                unknown_start = None;
            }
        }
        if start.elapsed() > timeout {
            anyhow::bail!("Timeout waiting for session to complete");
        }
        time::sleep(poll_interval).await;
    }
}

/// Assemble system and user prompts from the agent‑kit files.
async fn assemble_prompts(
    stream_id: &str,
    project_root: &Path,
) -> Result<(String, String), anyhow::Error> {
    let repo_root = find_repo_root(project_root);
    let kit_dir = repo_root.join("docs/agent-kit");
    if !kit_dir.exists() {
        anyhow::bail!("Agent-kit directory not found at {}. Please ensure docs/agent-kit exists in the repository root.", kit_dir.display());
    }

    let master_ctx = tokio::fs::read_to_string(kit_dir.join("AGENT_MASTER_CONTEXT.md")).await?;
    let contracts = tokio::fs::read_to_string(kit_dir.join("CONTRACTS_LOCKED.md")).await?;
    let test_instructions =
        tokio::fs::read_to_string(kit_dir.join("GLYIM_TEST_INSTRUCTIONS.md")).await?;

    let system_prompt = format!(
        "You are a helpful assistant. Output only code inside ```glyim-ops blocks.\n\n\
         ---\n{}\n---\n{}\n---\n{}",
        master_ctx, contracts, test_instructions
    );

    let brief = tokio::fs::read_to_string(kit_dir.join(format!("briefs/{}.md", stream_id))).await?;

    let streams_json = tokio::fs::read_to_string(kit_dir.join("streams.json")).await?;
    let streams: Vec<serde_json::Value> = serde_json::from_str(&streams_json)?;
    let stream_data = streams
        .iter()
        .find(|s| s["id"].as_str() == Some(stream_id))
        .ok_or_else(|| anyhow::anyhow!("Stream {} not found in streams.json", stream_id))?;
    let owned_crates: Vec<String> = stream_data["owned_crates"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let exec_config = crate::config::types::ExecutionConfig::default();
    let worktree_base = Path::new(&exec_config.worktree_base);
    let worktree_path = worktree_base.join(format!("stream-{}", stream_id));
    let source_root = if worktree_path.exists() {
        worktree_path
    } else {
        // Fallback to repository root
        repo_root
    };

    let mut source_context = String::new();
    for crate_name in owned_crates {
        let crate_src = source_root.join("crates").join(&crate_name).join("src");
        if crate_src.exists() {
            for entry in WalkDir::new(&crate_src)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
            {
                let path = entry.path();
                let rel_path = path.strip_prefix(&source_root).unwrap_or(path);
                // Skip test files except mod.rs
                if path.to_string_lossy().contains("/tests/")
                    && !path.to_string_lossy().ends_with("mod.rs")
                {
                    continue;
                }
                let content = tokio::fs::read_to_string(path).await?;
                source_context.push_str(&format!(
                    "\n### {}\n```rust\n{}\n```\n",
                    rel_path.display(),
                    content
                ));
            }
        }
    }

    let user_prompt = format!(
        "You are implementing Stream {} for the Glyim compiler.\n\n\
         ## Your Stream Brief\n{}\n\n\
         ## Source Code Context\n{}",
        stream_id, brief, source_context
    );

    Ok((system_prompt, user_prompt))
}

/// Run a wave: read streams.json, group by wave, start all streams in the current wave,
/// wait for them to finish, then proceed to the next wave when `--next` is used.
async fn run_wave(
    next: bool,
    config: &Arc<PilotConfig>,
    project_root: &Path,
) -> Result<(), anyhow::Error> {
    let repo_root = find_repo_root(project_root);
    let kit_dir = repo_root.join("docs/agent-kit");
    if !kit_dir.exists() {
        anyhow::bail!("Agent-kit directory not found at {}.", kit_dir.display());
    }

    let streams_json = tokio::fs::read_to_string(kit_dir.join("streams.json")).await?;
    let mut streams: Vec<serde_json::Value> = serde_json::from_str(&streams_json)?;

    let waves = compute_waves(&mut streams)?;

    let persistence = StatePersistence::load(project_root).await?;
    let mut current_wave = 0;
    for (i, wave_streams) in waves.iter().enumerate() {
        let mut all_merged = true;
        for sid in wave_streams {
            let merged = persistence.get_pr_merged(sid).await.unwrap_or(false);
            if !merged {
                all_merged = false;
                break;
            }
        }
        if !all_merged {
            current_wave = i;
            break;
        }
    }

    if next {
        println!("Proceeding to wave {}", current_wave);
        let wave_streams = &waves[current_wave];
        let mut provider_pool = ProviderPool::new(&config.providers);
        let assignments = dispatch_wave(
            wave_streams,
            &mut provider_pool,
            &DispatchStrategy::MostSlotsFirst,
        )?;
        for assignment in assignments {
            println!(
                "Starting stream {} on provider {}",
                assignment.stream_id, assignment.provider_id
            );
            let config_clone = config.clone();
            let project_root_owned = project_root.to_path_buf();
            tokio::spawn(async move {
                let _ = run_stream(
                    assignment.stream_id,
                    Some(assignment.provider_id),
                    config_clone,
                    project_root_owned,
                    true,
                )
                .await;
            });
        }
        println!(
            "Wave {} started. Use `cargo run -- agent wave --next` after all PRs are merged.",
            current_wave
        );
    } else {
        println!("Current wave: {}", current_wave);
        println!("Streams not yet merged:");
        for sid in &waves[current_wave] {
            let merged = persistence.get_pr_merged(sid).await.unwrap_or(false);
            if !merged {
                println!("  - {}", sid);
            }
        }
        println!("Run `cargo run -- agent wave --next` after merging all PRs in this wave.");
    }
    Ok(())
}

/// Compute wave numbers from upstream dependencies (topological sort).
fn compute_waves(streams: &mut [serde_json::Value]) -> Result<Vec<Vec<String>>, anyhow::Error> {
    let mut id_to_wave: HashMap<String, usize> = HashMap::new();
    let mut id_to_upstream: HashMap<String, Vec<String>> = HashMap::new();
    for stream in streams.iter() {
        let id = stream["id"].as_str().unwrap().to_string();
        let upstream: Vec<String> = stream["upstream"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        id_to_upstream.insert(id.clone(), upstream);
        id_to_wave.insert(id, 0);
    }

    let mut changed = true;
    while changed {
        changed = false;
        for (id, upstream) in id_to_upstream.iter() {
            let max_upstream_wave = upstream.iter().map(|u| id_to_wave[u]).max().unwrap_or(0);
            let new_wave = max_upstream_wave + 1;
            if new_wave > id_to_wave[id] {
                id_to_wave.insert(id.clone(), new_wave);
                changed = true;
            }
        }
    }

    let max_wave = *id_to_wave.values().max().unwrap_or(&0);
    let mut waves = vec![vec![]; max_wave + 1];
    for (id, wave) in id_to_wave.iter() {
        waves[*wave].push(id.clone());
    }

    for stream in streams.iter_mut() {
        let id = stream["id"].as_str().unwrap();
        if let Some(wave) = id_to_wave.get(id) {
            stream["wave"] = serde_json::Value::Number(serde_json::Number::from(*wave));
        }
    }

    Ok(waves)
}

/// Mark a session's PR as merged (manual fallback).
async fn mark_merged(
    stream_id: String,
    _config: &Arc<PilotConfig>,
    _project_root: &Path,
) -> Result<(), anyhow::Error> {
    let client = Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:8421/session/{}/merge", stream_id))
        .send()
        .await?;
    if resp.status().is_success() {
        println!("Stream {} marked as merged.", stream_id);
    } else {
        let text = resp.text().await?;
        anyhow::bail!("Failed to mark merged: {}", text);
    }
    Ok(())
}
