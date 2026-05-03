mod verify;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use glyim_macro_vfs::{ActionResult, ContentHash, ContentStore, LocalContentStore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

// ── Shared application state ───────────────────────────────────────
pub struct AppState {
    store: Mutex<LocalContentStore>,
}

// ── Request/Response types ─────────────────────────────────────────

#[derive(Deserialize)]
struct BlobHash {
    hash: String,
}

#[derive(Serialize)]
struct StoreResponse {
    hash: String,
}

#[derive(Deserialize)]
struct FindMissingRequest {
    blobs: Vec<String>,
}

#[derive(Serialize)]
struct FindMissingResponse {
    missing: Vec<String>,
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
    version: String,
    blob_count: usize,
}

// ── Handlers ───────────────────────────────────────────────────────

/// Store a blob and return its content hash.
async fn store_blob(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let store = state.store.lock().await;
    let hash = store.store(&body);
    tracing::info!("Stored blob: {}", hash);
    Json(StoreResponse {
        hash: hash.to_hex(),
    })
}

/// Retrieve a blob by its content hash.
async fn retrieve_blob(
    State(state): State<Arc<AppState>>,
    Path(hash_str): Path<String>,
) -> Response {
    let hash = match hash_str.parse::<ContentHash>() {
        Ok(h) => h,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "invalid hash").into_response();
        }
    };
    let store = state.store.lock().await;
    match store.retrieve(hash) {
        Some(data) => {
            tracing::info!("Retrieved blob: {}", hash);
            (StatusCode::OK, [(header::CONTENT_TYPE, "application/octet-stream")], data).into_response()
        }
        None => {
            tracing::info!("Blob not found: {}", hash);
            (StatusCode::NOT_FOUND, "blob not found").into_response()
        }
    }
}

/// Check which of the requested blobs are missing from the store.
async fn find_missing_blobs(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FindMissingRequest>,
) -> impl IntoResponse {
    let store = state.store.lock().await;
    let mut missing = Vec::new();
    for hash_str in &req.blobs {
        if let Ok(hash) = hash_str.parse::<ContentHash>() {
            if store.retrieve(hash).is_none() {
                missing.push(hash.to_hex());
            }
        } else {
            missing.push(hash_str.clone());
        }
    }
    tracing::info!(
        "FindMissingBlobs: {} requested, {} missing",
        req.blobs.len(),
        missing.len()
    );
    Json(FindMissingResponse { missing })
}

/// Store an action result (macro expansion result) under its action hash.
async fn store_action_result(
    State(state): State<Arc<AppState>>,
    Path(hash_str): Path<String>,
    Json(result): Json<ActionResult>,
) -> impl IntoResponse {
    let hash = match hash_str.parse::<ContentHash>() {
        Ok(h) => h,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid hash").into_response(),
    };
    let store = state.store.lock().await;
    match store.store_action_result(hash, result) {
        Ok(()) => {
            tracing::info!("Stored action result: {}", hash);
            (StatusCode::OK, "ok").into_response()
        }
        Err(e) => {
            tracing::error!("Failed to store action result: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("store error: {:?}", e),
            )
                .into_response()
        }
    }
}

/// Retrieve an action result by its action hash.
async fn retrieve_action_result(
    State(state): State<Arc<AppState>>,
    Path(hash_str): Path<String>,
) -> impl IntoResponse {
    let hash = match hash_str.parse::<ContentHash>() {
        Ok(h) => h,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid hash").into_response(),
    };
    let store = state.store.lock().await;
    match store.retrieve_action_result(hash) {
        Some(result) => {
            tracing::info!("Retrieved action result: {}", hash);
            Json(result).into_response()
        }
        None => {
            tracing::info!("Action result not found: {}", hash);
            (StatusCode::NOT_FOUND, "action result not found").into_response()
        }
    }
}

/// Health check and status endpoint.
async fn status(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let blob_count = std::fs::read_dir("./cas_store/objects")
        .ok()
        .map(|entries| entries.count())
        .unwrap_or(0);

    Json(StatusResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        blob_count,
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let store = LocalContentStore::new("./cas_store")
        .expect("failed to create local content store");
    let state = Arc::new(AppState {
        store: Mutex::new(store),
    });

    let app = Router::new()
        .route("/blob", post(store_blob))
        .route("/blob/{hash}", get(retrieve_blob))
        .route("/blob/missing", post(find_missing_blobs))
        .route("/action/{hash}", post(store_action_result).get(retrieve_action_result))
        .route("/status", get(status))
        .route("/verify-wasm", post(verify::verify_wasm))
        .with_state(state);

    let addr = "127.0.0.1:9090";
    tracing::info!("Starting glyim-cas-server on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
