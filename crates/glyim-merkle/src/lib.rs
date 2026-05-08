pub mod node;
pub mod store;
pub mod root;

pub use node::{MerkleNode, MerkleNodeData, MerkleNodeHeader};
pub use store::MerkleStore;
pub use root::{MerkleRoot, compute_root_hash, ItemChange};

#[cfg(test)]
mod tests;
