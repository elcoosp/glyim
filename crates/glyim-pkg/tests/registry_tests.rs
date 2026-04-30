use glyim_pkg::registry::*;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn registry_client_new_with_url() {
    let client = RegistryClient::new("https://registry.glyim.dev").unwrap();
    assert_eq!(client.endpoint(), "https://registry.glyim.dev");
}

#[test]
fn registry_client_fetch_returns_error_on_bad_url() {
    let client = RegistryClient::new("http://localhost:99999").unwrap();
    let result = client.fetch_available("nonexistent");
    assert!(result.is_err());
}

#[tokio::test]
async fn wiremock_publish_endpoint_works() {
    // Verify that the mock server intercepts a POST and returns 200
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/api/v1/packages/test-pkg/1.0.0/upload"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let url = format!("{}/api/v1/packages/test-pkg/1.0.0/upload", server.uri());
    let client = reqwest::Client::new();
    let resp = client.post(&url)
        .header("Content-Type", "application/octet-stream")
        .body(b"fake-tarball-content".to_vec())
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn wiremock_fetch_available_works() {
    // Verify that the mock server returns expected JSON
    let server = MockServer::start().await;
    let response_body = serde_json::json!({
        "name": "test-pkg",
        "versions": [
            {"version": "1.0.0", "is_macro": false, "deps": []},
            {"version": "1.2.0", "is_macro": false, "deps": []},
            {"version": "2.0.0-beta", "is_macro": false, "deps": []}
        ]
    });

    Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/api/v1/packages/test-pkg"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .mount(&server)
        .await;

    let url = format!("{}/api/v1/packages/test-pkg", server.uri());
    let resp = reqwest::get(&url).await.unwrap();
    assert!(resp.status().is_success());
    let json: serde_json::Value = resp.json().await.unwrap();
    let versions = json["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!(versions[2]["version"], "2.0.0-beta");
}
