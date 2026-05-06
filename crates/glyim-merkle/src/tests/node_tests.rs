use crate::node::{MerkleNode, MerkleNodeData, MerkleNodeHeader};
use glyim_macro_vfs::ContentHash;
use sha2::{Digest, Sha256};

/// Build a real MerkleNode and verify that the hash in the node matches
/// the computed hash from data + children (the "hash dependency").
#[test]
fn hash_dependency() {
    let data = MerkleNodeData::HirFn {
        name: "add".into(),
        serialized: vec![1, 2, 3],
    };
    let header = MerkleNodeHeader {
        data_type_tag: data.data_type_tag(),
        child_count: 1,
    };
    let child = ContentHash::of(b"child");

    // Compute the expected hash using the same method as MerkleNode::compute_hash()
    let expected_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&(1u64.to_le_bytes()));  // child count
        hasher.update(child.as_bytes());
        // reconstruct the exact data blob produced by serialize_data()
        let mut data_blob = vec![0x01u8];       // DATA_TYPE_HIR_FN
        let name_bytes = b"add";
        data_blob.extend_from_slice(&(name_bytes.len() as u64).to_le_bytes());
        data_blob.extend_from_slice(name_bytes);
        let ser_bytes = &[1, 2, 3];
        data_blob.extend_from_slice(&(ser_bytes.len() as u64).to_le_bytes());
        data_blob.extend_from_slice(ser_bytes);
        hasher.update(&data_blob);
        let digest = hasher.finalize();
        ContentHash::from_bytes(digest.into())
    };

    let node = MerkleNode {
        hash: expected_hash,       // the node carries the correct hash
        children: vec![child],
        data,
        header,
    };

    // The node's own compute_hash() must match the stored hash
    assert_eq!(node.compute_hash(), expected_hash);
    assert_eq!(node.hash, expected_hash);
}

#[test]
fn compute_hash_deterministic() {
    let data = MerkleNodeData::HirFn { name: "x".into(), serialized: vec![42] };
    let header = MerkleNodeHeader { data_type_tag: 1, child_count: 0 };
    let n1 = MerkleNode { hash: ContentHash::ZERO, children: vec![], data: data.clone(), header: header.clone() };
    let n2 = MerkleNode { hash: ContentHash::ZERO, children: vec![], data, header };
    assert_eq!(n1.compute_hash(), n2.compute_hash());
}

#[test]
fn different_content_different_hash() {
    let a = MerkleNode {
        hash: ContentHash::ZERO, children: vec![],
        data: MerkleNodeData::HirFn { name: "a".into(), serialized: vec![1] },
        header: MerkleNodeHeader { data_type_tag: 1, child_count: 0 },
    };
    let b = MerkleNode {
        hash: ContentHash::ZERO, children: vec![],
        data: MerkleNodeData::HirFn { name: "b".into(), serialized: vec![2] },
        header: MerkleNodeHeader { data_type_tag: 1, child_count: 0 },
    };
    assert_ne!(a.compute_hash(), b.compute_hash());
}

#[test]
fn children_affect_hash() {
    let data = MerkleNodeData::HirFn { name: "a".into(), serialized: vec![1] };
    let header = MerkleNodeHeader { data_type_tag: 1, child_count: 0 };
    let a = MerkleNode { hash: ContentHash::ZERO, children: vec![], data: data.clone(), header: header.clone() };
    let b = MerkleNode { hash: ContentHash::ZERO, children: vec![ContentHash::of(b"c")], data, header };
    assert_ne!(a.compute_hash(), b.compute_hash());
}
