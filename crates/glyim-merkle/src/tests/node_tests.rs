use glyim_merkle::node::{MerkleNode, MerkleNodeData, MerkleNodeHeader};
use glyim_macro_vfs::ContentHash;

fn make_node(name: &str, serialized: Vec<u8>, children: Vec<ContentHash>) -> MerkleNode {
    let data = MerkleNodeData::HirFn { name: name.to_string(), serialized };
    let header = MerkleNodeHeader { data_type_tag: data.data_type_tag(), child_count: children.len() as u32 };
    let hash = {
        let mut hasher = sha2::Sha256::new();
        hasher.update(&(children.len() as u64).to_le_bytes());
        for c in &children { hasher.update(c.as_bytes()); }
        hasher.update(&vec![0x01]); // mimic serialize_data minimal
        let d = hasher.finalize();
        ContentHash::from_bytes(d.into())
    };
    MerkleNode { hash, children, data, header }
}

#[test]
fn serialize_roundtrip_hirfn() {
    let node = make_node("add", vec![1,2,3], vec![ContentHash::of(b"child")]);
    let serialized = node.serialize();
    let restored = MerkleNode::deserialize(&serialized).unwrap();
    assert!(matches!(restored.data, MerkleNodeData::HirFn { name, .. } if name == "add"));
    assert_eq!(restored.children.len(), 1);
}

#[test]
fn compute_hash_deterministic() {
    let n1 = make_node("x", vec![42], vec![]);
    let n2 = make_node("x", vec![42], vec![]);
    assert_eq!(n1.compute_hash(), n2.compute_hash());
}

#[test]
fn different_content_different_hash() {
    let a = make_node("a", vec![1], vec![]);
    let b = make_node("b", vec![2], vec![]);
    assert_ne!(a.compute_hash(), b.compute_hash());
}

#[test]
fn children_affect_hash() {
    let a = make_node("a", vec![1], vec![]);
    let b = make_node("a", vec![1], vec![ContentHash::of(b"c")]);
    assert_ne!(a.compute_hash(), b.compute_hash());
}
