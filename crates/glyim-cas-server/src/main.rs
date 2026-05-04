mod grpc;
mod verify;

use axum::{
    Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use glyim_macro_vfs::{ActionResult, ContentHash, ContentStore, LocalContentStore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

// ── Shared application state ───────────────────────────────────────
pub struct AppState {
    store: Arc<RwLock<LocalContentStore>>,
}

// ── Request/Response types ─────────────────────────────────────────

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
    let store = state.store.read().await;
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
    let store = state.store.read().await;
    match store.retrieve(hash) {
        Some(data) => {
            tracing::info!("Retrieved blob: {}", hash);
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/octet-stream")],
                data,
            )
                .into_response()
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
    let store = state.store.read().await;
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
    let store = state.store.read().await;
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
    let store = state.store.read().await;
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let raw_store =
        LocalContentStore::new("./cas_store").expect("failed to create local content store");
    let shared_store = Arc::new(RwLock::new(raw_store));

    let state = Arc::new(AppState {
        store: shared_store.clone(),
    });

    // ── REST server on port 9090 ──────────────────────────────────
    let rest_app = Router::new()
        .route("/blob", post(store_blob))
        .route("/blob/{hash}", get(retrieve_blob))
        .route("/blob/missing", post(find_missing_blobs))
        .route(
            "/action/{hash}",
            post(store_action_result).get(retrieve_action_result),
        )
        .route("/status", get(status))
        .route("/verify-wasm", post(verify::verify_wasm))
        .with_state(state);

    let rest_addr = "127.0.0.1:9090";
    tracing::info!("Starting REST server on http://{}", rest_addr);
    let rest_listener = tokio::net::TcpListener::bind(rest_addr).await?;
    let rest_handle = tokio::spawn(async move {
        axum::serve(rest_listener, rest_app).await.expect("internal error");
    });

    // ── gRPC server on port 9091 ─────────────────────────────────
    let cas_service = grpc::cas::CasService {
        store: shared_store.clone(),
    };
    let capabilities_service = grpc::capabilities::CapabilitiesService::default();

    let grpc_addr = "127.0.0.1:9091".parse().expect("internal error");
    tracing::info!("Starting gRPC server on {}", grpc_addr);

    let grpc_handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(
                bazel_remote_apis::build::bazel::remote::execution::v2::capabilities_server::CapabilitiesServer::new(capabilities_service),
            )
            .add_service(
                bazel_remote_apis::build::bazel::remote::execution::v2::content_addressable_storage_server::ContentAddressableStorageServer::new(cas_service),
            )
            .serve(grpc_addr)
            .await
            .expect("internal error");
    });

    // Wait for both servers
    tokio::select! {
        _ = rest_handle => {},
        _ = grpc_handle => {},
    }

    Ok(())
}
