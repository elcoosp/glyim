//! Endpoints for verifying Wasm reproducibility.
//!
//! The registry re‑compiles a macro from its source and checks that the
//! resulting `.wasm` blob matches the hash declared by the publisher.
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use glyim_macro_vfs::{ContentHash, ContentStore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Deserialize)]
pub struct VerifyWasmRequest {
    pub source: String,        // the macro source code
    pub expected_hash: String, // hex‑encoded hash of the published Wasm blob
}

#[derive(Serialize)]
pub struct VerifyWasmResponse {
    pub matches: bool,
    pub actual_hash: Option<String>,
    pub error: Option<String>,
}

/// POST /verify-wasm
/// Re‑compile the given source to wasm32‑wasi and check that the hash matches.
pub async fn verify_wasm(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VerifyWasmRequest>,
) -> impl IntoResponse {
    let expected = match req.expected_hash.parse::<ContentHash>() {
        Ok(h) => h,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(VerifyWasmResponse {
                    matches: false,
                    actual_hash: None,
                    error: Some("invalid hash format".into()),
                }),
            );
        }
    };

    // Compile the source to Wasm
    let wasm_bytes = match glyim_codegen_llvm::compile_to_wasm(&req.source, "wasm32-wasi") {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::OK,
                Json(VerifyWasmResponse {
                    matches: false,
                    actual_hash: None,
                    error: Some(format!("compilation error: {e}")),
                }),
            );
        }
    };

    let actual_hash = ContentHash::of(&wasm_bytes);
    let matches = actual_hash == expected;

    // If verification passes, also store the blob in the CAS
    if matches {
        let _ = state.store.write().await.store(&wasm_bytes);
    }

    (
        StatusCode::OK,
        Json(VerifyWasmResponse {
            matches,
            actual_hash: Some(actual_hash.to_hex()),
            error: None,
        }),
    )
}
