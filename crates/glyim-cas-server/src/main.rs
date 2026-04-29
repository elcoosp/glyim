use actix_web::{web, App, HttpServer, HttpResponse, Result};
use glyim_macro_vfs::{ContentHash, LocalContentStore, ContentStore};
use std::sync::Mutex;

struct AppState {
    store: Mutex<LocalContentStore>,
}

async fn store_blob(
    state: web::Data<AppState>,
    bytes: web::Bytes,
) -> Result<HttpResponse> {
    let store = state.store.lock().unwrap();
    let hash = store.store(&bytes);
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "hash": hash.to_hex()
    })))
}

async fn retrieve_blob(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let hash_str = path.into_inner();
    let hash = hash_str.parse::<ContentHash>().map_err(|_| {
        actix_web::error::ErrorBadRequest("invalid hash")
    })?;
    let store = state.store.lock().unwrap();
    match store.retrieve(hash) {
        Some(data) => Ok(HttpResponse::Ok().body(data)),
        None => Ok(HttpResponse::NotFound().finish()),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let store = LocalContentStore::new("./cas_store")?;
    let data = web::Data::new(AppState {
        store: Mutex::new(store),
    });

    println!("Starting glyim-cas-server on http://127.0.0.1:9090");
    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .route("/blob", web::post().to(store_blob))
            .route("/blob/{hash}", web::get().to(retrieve_blob))
    })
    .bind("127.0.0.1:9090")?
    .run()
    .await
}
