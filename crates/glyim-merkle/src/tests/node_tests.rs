use crate::node::{MerkleNode, MerkleNodeData, MerkleNodeHeader};
use glyim_macro_vfs::ContentHash;

#[test]
fn serialize_roundtrip_hirfn() {
    let data = MerkleNodeData::HirFn {
        name: "add".to_string(),
        serialized: vec![1, 2, 3],
    };
    let header = MerkleNodeHeader {
        data_type_tag: data.data_type_tag(),
        child_count: 1,
    };
    let child = ContentHash::of(b"child");
    // Compute the real hash from data+children
    let hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&1u64.to_le_bytes()); // child count
        hasher.update(child.as_bytes());
        // data blob (must match serialize_data output)
        let mut data_blob = vec![0x01u8]; // HirFn tag
        data_blob.extend_from_slice(&3u64.to_le_bytes()); // name len
        data_blob.extend_from_slice(b"add");
        data_blob.extend_from_slice(&3u64.to_le_bytes()); // serialized len
        data_blob.extend_from_slice(&[1, 2, 3]);
        hasher.update(&data_blob);
        let digest = hasher.finalize();
        ContentHash::from_bytes(digest.into())
    };
    let node = MerkleNode {
        hash,
        children: vec![child],
        data,
        header,
    };
    let serialized = node.serialize();
    let restored = MerkleNode::deserialize(&serialized).expect("deserialize");
    // Verify the hash matches (dependency)
    assert_eq!(restored.hash, node.hash);
    assert_eq!(restored.children.len(), 1);
    match restored.data {
        MerkleNodeData::HirFn { name, .. } => assert_eq!(name, "add"),
        _ => panic!("wrong data type"),
    }
}

#[test]
fn compute_hash_deterministic() {
    let data = MerkleNodeData::HirFn { name: "x".into(), serialized: vec![42] };
    let header = MerkleNodeHeader { data_type_tag: 1, child_count: 0 };
    let empty = vec![];
    let node1 = MerkleNode { hash: ContentHash::ZERO, children: empty.clone(), data: data.clone(), header: header.clone() };
    let node2 = MerkleNode { hash: ContentHash::ZERO, children: empty.clone(), data, header };
    assert_eq!(node1.compute_hash(), node2.compute_hash());
}

#[test]
fn different_content_different_hash() {
    let a = MerkleNode {
        hash: ContentHash::ZERO,
        children: vec![],
        data: MerkleNodeData::HirFn { name: "a".into(), serialized: vec![1] },
        header: MerkleNodeHeader { data_type_tag: 1, child_count: 0 },
    };
    let b = MerkleNode {
        hash: ContentHash::ZERO,
        children: vec![],
        data: MerkleNodeData::HirFn { name: "b".into(), serialized: vec![2] },
        header: MerkleNodeHeader { data_type_tag: 1, child_count: 0 },
    };
    assert_ne!(a.compute_hash(), b.compute_hash());
}

#[test]
fn children_affect_hash() {
    let data = MerkleNodeData::HirFn { name: "a".into(), serialized: vec![1] };
    let header = MerkleNodeHeader { data_type_tag: 1, child_count: 0 };
    let a = MerkleNode {
        hash: ContentHash::ZERO, children: vec![],
        data: data.clone(), header: header.clone(),
    };
    let b = MerkleNode {
        hash: ContentHash::ZERO, children: vec![ContentHash::of(b"c")],
        data, header,
    };
    assert_ne!(a.compute_hash(), b.compute_hash());
}
