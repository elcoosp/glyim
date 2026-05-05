#![allow(unused_imports, dead_code)]
use glyim_pkg::cas_client::CasClient;

#[test]
fn store_and_retrieve() {
    let dir = tempfile::tempdir().unwrap();
    let client = CasClient::new(dir.path()).unwrap();
    let content = b"hello, cas!";
    let hash = client.store(content);
    let retrieved = client.retrieve(hash);
    assert_eq!(retrieved, Some(content.to_vec()));
    assert!(client.contains(hash));
}

#[test]
fn store_empty_content() {
    let dir = tempfile::tempdir().unwrap();
    let client = CasClient::new(dir.path()).unwrap();
    let hash = client.store(b"");
    let retrieved = client.retrieve(hash);
    assert_eq!(retrieved, Some(vec![]));
}

#[test]
fn different_content_different_hash() {
    let dir = tempfile::tempdir().unwrap();
    let client = CasClient::new(dir.path()).unwrap();
    let hash1 = client.store(b"abc");
    let hash2 = client.store(b"def");
    assert_ne!(hash1, hash2);
}

#[test]
fn register_and_resolve_name() {
    let dir = tempfile::tempdir().unwrap();
    let client = CasClient::new(dir.path()).unwrap();
    let content = b"named content";
    let hash = client.store(content);
    client.register_name("my-package", hash);
    let resolved = client.resolve_name("my-package");
    assert_eq!(resolved, Some(hash));
}

#[test]
fn download_from_remote_is_stub() {
    let dir = tempfile::tempdir().unwrap();
    let client = CasClient::new(dir.path()).unwrap();
    let hash = client.store(b"data");
    let result = client.download_from_remote(hash, "http://localhost:9090");
    assert!(result.is_err());
}
