use clap::{Parser, Subcommand};
use glyim_pilot::cli::agent::{handle_agent_command, AgentCommands};
use glyim_pilot::cli::session::{handle_session_command, SessionCommands};
use glyim_pilot::cli::{render_status_table, run_preflight};
use glyim_pilot::config::{self, PilotConfig};
use glyim_pilot::metrics::production_metrics;
use glyim_pilot::protocol::types::PROTOCOL_VERSION;
use glyim_pilot::server::{CliMessage, ExtensionMessage, ServerEvent, WsServer};
use glyim_pilot::session::persistence::StatePersistence;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

// HTTP server dependencies
use axum::extract::Path as AxumPath;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Parser)]
#[command(name = "glyim-pilot", version = "0.3.0")]
struct Cli {
    #[arg(long, env = "GLYIM_PROJECT_ROOT", default_value = ".")]
    project_root: PathBuf,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Serve,
    Status,
    Preflight,
    #[command(subcommand)]
    Session(SessionCommands),
    #[command(subcommand)]
    Agent(AgentCommands),
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let config = match config::load_config(&cli.project_root) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("Config error: {e}");
            std::process::exit(1);
        }
    };

    match cli.command {
        Commands::Serve => run_serve(config, cli.project_root).await,
        Commands::Status => run_status(cli.project_root).await,
        Commands::Preflight => run_preflight(&config).await,
        Commands::Session(cmd) => {
            if let Err(e) = handle_session_command(cmd).await {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Agent(cmd) => {
            if let Err(e) = handle_agent_command(cmd, &config, &cli.project_root).await {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}

async fn run_serve(config: Arc<PilotConfig>, project_root: PathBuf) {
    let mut server = WsServer::new(&config.server.host, config.server.port);
    let mut event_rx = server.take_event_rx().expect("event rx already taken");
    let cli_sender = server.cli_msg_sender();
    let server = Arc::new(server);
    let server_clone = Arc::clone(&server);
    tokio::spawn(async move {
        if let Err(e) = server_clone.run().await {
            tracing::error!("Server error: {e}");
        }
    });

    // Keep broadcast channel alive with a dummy receiver
    let mut _dummy_rx = cli_sender.subscribe();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        }
    });

    let persistence = Arc::new(
        StatePersistence::load(&project_root)
            .await
            .expect("failed to load state"),
    );
    let processing: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let metrics: Arc<dyn glyim_pilot::metrics::Metrics> = production_metrics().into();

    tracing::info!(
        "Glyim Pilot server started on ws://{}:{}",
        config.server.host,
        config.server.port
    );

    // HTTP server for CLI commands – use a tuple as shared state
    let app = Router::new()
        .route("/session/start", post(session_start_handler))
        .route("/session/{id}/status", get(session_status_handler))
        .route("/session/{id}/merge", post(session_merge_handler))
        .with_state((cli_sender.clone(), persistence.clone()));

    let http_addr = SocketAddr::from(([127, 0, 0, 1], 8421));
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("HTTP server error: {e}");
        }
    });

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => { tracing::info!("Shutting down..."); break; }
            Some(event) = event_rx.recv() => {
                match event {
                    ServerEvent::Connected { addr } => {
                        tracing::info!(peer = %addr, "extension connected");
                    }
                    ServerEvent::Disconnected { addr } => tracing::info!(peer = %addr, "extension disconnected"),
                    ServerEvent::Message { msg, .. } => {
                        handle_extension_message(
                            msg, &config, &persistence, &project_root,
                            &cli_sender, &processing, &metrics,
                        ).await;
                    }
                }
            }
        }
    }
}

async fn session_start_handler(
    State((sender, _)): State<(
        tokio::sync::broadcast::Sender<String>,
        Arc<StatePersistence>,
    )>,
    Json(payload): Json<SessionStartRequest>,
) -> impl IntoResponse {
    let system_prompt = payload.system_prompt.unwrap_or_else(|| {
        "You are a helpful assistant. Output only code inside ```glyim-ops blocks.".to_string()
    });
    let msg = CliMessage::SessionStart {
        session_id: payload
            .session_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        provider_id: payload.provider,
        prompt: payload.prompt,
        system_prompt,
        trace_id: None,
        v: PROTOCOL_VERSION,
    };
    let json = match serde_json::to_string(&msg) {
        Ok(j) => j,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    if let Err(e) = sender.send(json) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    (StatusCode::OK, "Session start sent").into_response()
}

#[derive(Deserialize)]
struct SessionStartRequest {
    provider: String,
    prompt: String,
    session_id: Option<String>,
    system_prompt: Option<String>,
}

async fn session_status_handler(
    AxumPath(id): AxumPath<String>,
    State((_, persistence)): State<(
        tokio::sync::broadcast::Sender<String>,
        Arc<StatePersistence>,
    )>,
) -> impl IntoResponse {
    let status = persistence
        .get_session_status(&id)
        .await
        .unwrap_or_else(|| "unknown".to_string());
    Json(serde_json::json!({ "status": status }))
}

async fn session_merge_handler(
    AxumPath(id): AxumPath<String>,
    State((_, persistence)): State<(
        tokio::sync::broadcast::Sender<String>,
        Arc<StatePersistence>,
    )>,
) -> impl IntoResponse {
    match persistence.mark_pr_merged(&id).await {
        Ok(_) => (StatusCode::OK, "Merged".to_string()).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    }
}

async fn handle_extension_message(
    msg: ExtensionMessage,
    config: &Arc<PilotConfig>,
    persistence: &Arc<StatePersistence>,
    project_root: &Path,
    cli_sender: &tokio::sync::broadcast::Sender<String>,
    processing: &Arc<Mutex<HashSet<String>>>,
    metrics: &Arc<dyn glyim_pilot::metrics::Metrics>,
) {
    match msg {
        ExtensionMessage::SessionReady {
            session_id,
            provider_id,
            tab_id,
            ..
        } => {
            tracing::info!(session_id, provider_id, tab_id, "session ready");
        }
        ExtensionMessage::OpsReady {
            session_id,
            content,
            turn,
            trace_id,
            ..
        } => {
            let trace_id = trace_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            let provider_id = persistence
                .all_sessions()
                .await
                .into_iter()
                .find(|s| s.session_id == session_id)
                .map(|s| s.provider_id)
                .unwrap_or_else(|| config.defaults.provider.clone());

            let worktree_dir = match persistence.get_worktree_path(&session_id).await {
                Some(path) if !path.is_empty() => PathBuf::from(path),
                _ => {
                    tracing::info!(session_id, "worktree not found or empty, creating one");
                    let stream_id = session_id.clone();
                    let worktree_base = Path::new(&config.execution.worktree_base);
                    let repo_root = project_root;
                    let default_branch = &config.execution.default_branch;
                    let branch_version = &config.execution.branch_version;
                    let timeout = config.execution.command_timeout;

                    let worktree_dir = match glyim_pilot::git_ops::create_worktree(
                        repo_root,
                        worktree_base,
                        &stream_id,
                        default_branch,
                        branch_version,
                        timeout,
                    )
                    .await
                    {
                        Ok(dir) => dir,
                        Err(e) => {
                            tracing::error!(session_id, error = %e, "failed to create worktree");
                            let err_msg = CliMessage::FeedbackSend {
                                session_id: session_id.clone(),
                                message: format!("Worktree creation failed: {}", e),
                                turn: turn + 1,
                                trace_id: Some(trace_id.clone()),
                                v: PROTOCOL_VERSION,
                            };
                            let _ = cli_sender.send(serde_json::to_string(&err_msg).unwrap());
                            return;
                        }
                    };

                    let session_state = glyim_pilot::session::state::SessionState::new(
                        stream_id.clone(),
                        provider_id.clone(),
                        worktree_dir.to_string_lossy().to_string(),
                    );
                    if let Err(e) = persistence.add_session(session_state).await {
                        tracing::error!("failed to save session state: {}", e);
                    }
                    worktree_dir
                }
            };

            let stream_id = persistence
                .get_stream_id(&session_id)
                .await
                .unwrap_or_else(|| session_id.clone());

            let turn_ctx = glyim_pilot::orchestrator::TurnContext {
                ops_block: content,
                session_id,
                stream_id,
                worktree_dir,
                project_root: project_root.to_path_buf(),
                config: Arc::clone(config),
                persistence: Arc::clone(persistence),
                processing: Arc::clone(processing),
                turn,
                trace_id,
                metrics: Arc::clone(metrics),
            };

            let cli_sender_clone = cli_sender.clone();
            let metrics_clone = Arc::clone(metrics);
            // T151-PATCHED [PILOT-9]: capture the session id and trace id
            // for the error path before `turn_ctx` moves them into its
            // fields. The closure below can then build a FeedbackSend
            // message when the orchestrator returns Err.
            let session_id_for_err = turn_ctx.session_id.clone();
            // `CliMessage::FeedbackSend::trace_id` is `Option<String>`, and
            // `TurnContext::trace_id` is also `Option<String>` — clone it
            // as-is so the err path reuses the same shape the worktree path
            // above does.
            let trace_id_for_err = turn_ctx.trace_id.clone();

            tokio::spawn(async move {
                metrics_clone.increment_counter("ops_ready_received", &[]);

                match glyim_pilot::orchestrator::process_turn_dispatch(turn_ctx).await {
                    Ok(action) => {
                        if let Some(cli_msg) =
                            glyim_pilot::server::event_handler::map_action_to_cli_message(
                                action, turn,
                            )
                        {
                            let json = serde_json::to_string(&cli_msg).unwrap();
                            if let Err(e) = cli_sender_clone.send(json) {
                                tracing::warn!("failed to send CLI message: {e}");
                            }
                        } else {
                            tracing::debug!(
                                "orchestrator waiting for response — no CLI message needed"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!(?e, "orchestrator error");
                        metrics_clone
                            .increment_counter("orchestrator_error", &[("code", e.code())]);
                        // T151-PATCHED [PILOT-9]: forward the failure as
                        // feedback to the AI, matching the worktree-creation
                        // path above. Without this the extension is left
                        // waiting for a feedback message that never comes,
                        // and the turn deadlocks until human intervention.
                        // Recoverable classes (Parse / Apply / Limits /
                        // Gate) are safe to retry; anything else escalates.
                        let recoverable = matches!(
                            e,
                            glyim_pilot::error::PilotError::Parse { .. }
                                | glyim_pilot::error::PilotError::Apply(_)
                                | glyim_pilot::error::PilotError::Limits(_)
                                | glyim_pilot::error::PilotError::Gate { .. }
                        );
                        if recoverable {
                            let fb = CliMessage::FeedbackSend {
                                session_id: session_id_for_err.clone(),
                                message: format!(
                                    "Orchestrator error [{}]: {}. Fix your glyim-ops block and resend.",
                                    e.code(),
                                    e
                                ),
                                turn: turn + 1,
                                trace_id: Some(trace_id_for_err.clone()),
                                v: PROTOCOL_VERSION,
                            };
                            if let Ok(json) = serde_json::to_string(&fb) {
                                if let Err(send_err) = cli_sender_clone.send(json) {
                                    tracing::warn!("failed to send feedback: {send_err}");
                                }
                            }
                        }
                    }
                }
            });
        }
        ExtensionMessage::StreamComplete {
            session_id, turn, ..
        } => {
            tracing::info!(session_id, turn, "stream complete");
            metrics.increment_counter("stream_complete", &[]);
        }
        ExtensionMessage::ErrorDetected {
            session_id,
            error_type,
            error_message,
            recoverable,
            trace_id,
            ..
        } => {
            tracing::warn!(
                session_id,
                error_type,
                error_message,
                recoverable,
                "error from extension"
            );
            metrics.increment_counter("extension_error", &[("type", &error_type)]);
            if recoverable {
                let response = CliMessage::FeedbackSend {
                    session_id: session_id.clone(),
                    message: format!("Provider error: {}", error_message),
                    turn: 0,
                    trace_id,
                    v: PROTOCOL_VERSION,
                };
                let _ = cli_sender.send(serde_json::to_string(&response).unwrap());
            }
        }
        ExtensionMessage::Pong { timestamp, .. } => {
            tracing::debug!(timestamp, "pong");
        }
    }
}

async fn run_status(project_root: PathBuf) {
    let persistence = StatePersistence::load(&project_root)
        .await
        .expect("failed to load state");
    let sessions = persistence.all_sessions().await;
    if sessions.is_empty() {
        println!("No sessions found.");
    } else {
        println!("{}", render_status_table(&sessions));
    }
}
