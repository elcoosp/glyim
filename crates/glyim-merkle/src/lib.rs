pub mod node;
pub mod root;
pub mod store;

pub use node::{MerkleNode, MerkleNodeData, MerkleNodeHeader};
pub use root::{ItemChange, MerkleRoot, compute_root_hash};
pub use store::MerkleStore;

#[cfg(test)]
mod tests;
