use glyim_query::query_key::QueryKey;
use glyim_query::fingerprint::Fingerprint;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ParseFileKey {
    path: PathBuf,
}

impl QueryKey for ParseFileKey {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of_str(&self.path.to_string_lossy())
    }
}

#[test]
fn query_key_fingerprint_is_deterministic() {
    let key = ParseFileKey { path: PathBuf::from("main.g") };
    let fp1 = key.fingerprint();
    let fp2 = key.fingerprint();
    assert_eq!(fp1, fp2);
}

#[test]
fn query_key_different_keys_different_fingerprints() {
    let key_a = ParseFileKey { path: PathBuf::from("a.g") };
    let key_b = ParseFileKey { path: PathBuf::from("b.g") };
    assert_ne!(key_a.fingerprint(), key_b.fingerprint());
}

#[test]
fn query_key_trait_is_object_safe_for_bounds() {
    fn assert_bounds<K: QueryKey>() {}
    assert_bounds::<ParseFileKey>();
}

#[test]
fn query_key_implements_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<ParseFileKey>();
}
