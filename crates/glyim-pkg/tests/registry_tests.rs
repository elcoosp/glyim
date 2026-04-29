use glyim_pkg::registry::*;

#[test]
fn registry_client_new() {
    let client = RegistryClient::new("https://registry.glyim.dev");
    assert!(client.is_ok());
}

#[test]
fn registry_client_empty_url() {
    let client = RegistryClient::new("");
    assert!(client.is_ok());
}

#[test]
fn fetch_available_is_stub() {
    let client = RegistryClient::new("https://registry.glyim.dev").unwrap();
    let result = client.fetch_available("serde");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not yet implemented"));
}

#[test]
fn download_is_stub() {
    let client = RegistryClient::new("https://registry.glyim.dev").unwrap();
    let result = client.download("pkg", "1.0.0", std::path::Path::new("/tmp"));
    assert!(result.is_err());
}

#[test]
fn publish_is_stub() {
    let client = RegistryClient::new("https://registry.glyim.dev").unwrap();
    let result = client.publish(std::path::Path::new("/tmp/archive.tar.gz"));
    assert!(result.is_err());
}
