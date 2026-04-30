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
#[ignore]
    async fn publish_sends_data_to_registry() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/api/v1/packages/test-pkg/1.0.0/upload"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let client = RegistryClient::new(&server.uri()).unwrap();
    let result = client.publish("test-pkg", "1.0.0", b"fake-tarball-content");
    assert!(result.is_ok());
}

#[tokio::test]
#[ignore]
    async fn get_latest_version_returns_max_semver() {
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
        .expect(1)
        .mount(&server)
        .await;

    let client = RegistryClient::new(&server.uri()).unwrap();
    let latest = client.get_latest_version("test-pkg").unwrap();
    assert_eq!(latest, Some("2.0.0-beta".to_string()));
}
