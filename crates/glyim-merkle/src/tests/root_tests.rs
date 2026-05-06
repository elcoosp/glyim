use glyim_merkle::root::{MerkleRoot, compute_root_hash, ItemChange};
use glyim_macro_vfs::ContentHash;

#[test]
fn empty_root() {
    let hash = compute_root_hash(&[]);
    assert_ne!(hash, ContentHash::ZERO);
}

#[test]
fn single_item() {
    let h = ContentHash::of(b"fn_add");
    let root = compute_root_hash(&[("add".into(), h)]);
    assert_ne!(root, h);
}

#[test]
fn order_matters() {
    let a = ContentHash::of(b"a");
    let b = ContentHash::of(b"b");
    let r1 = compute_root_hash(&[("a".into(), a), ("b".into(), b)]);
    let r2 = compute_root_hash(&[("b".into(), b), ("a".into(), a)]);
    assert_ne!(r1, r2);
}

#[test]
fn find_changed_items_modified() {
    let old = vec![("x".into(), ContentHash::of(b"v1"))];
    let new = vec![("x".into(), ContentHash::of(b"v2"))];
    let changes = MerkleRoot::find_changed_items(&old, &new);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0], ("x".to_string(), ItemChange::Modified));
}

#[test]
fn find_changed_items_added() {
    let old = vec![];
    let new = vec![("y".into(), ContentHash::of(b"v"))];
    let changes = MerkleRoot::find_changed_items(&old, &new);
    assert_eq!(changes[0], ("y".to_string(), ItemChange::Added));
}

#[test]
fn find_changed_items_removed() {
    let old = vec![("z".into(), ContentHash::of(b"v"))];
    let new = vec![];
    let changes = MerkleRoot::find_changed_items(&old, &new);
    assert_eq!(changes[0], ("z".to_string(), ItemChange::Removed));
}
