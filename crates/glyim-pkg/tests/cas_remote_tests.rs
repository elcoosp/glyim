use glyim_pkg::cas_client::CasClient;

#[test]
fn cas_client_new_with_remote_creates_client() {
    let dir = tempfile::tempdir().unwrap();
    let client = CasClient::new_with_remote(dir.path(), "http://localhost:9090", None);
    assert!(client.is_ok());
}

#[test]
fn cas_client_new_with_remote_stores_locally() {
    let dir = tempfile::tempdir().unwrap();
    let client = CasClient::new_with_remote(
        dir.path(),
        "http://localhost:99999", // bad remote, should still work locally
        None,
    )
    .unwrap();
    let hash = client.store(b"hello remote");
    assert_eq!(client.retrieve(hash), Some(b"hello remote".to_vec()));
}
