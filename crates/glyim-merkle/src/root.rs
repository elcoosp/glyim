use glyim_macro_vfs::ContentHash;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub struct MerkleRoot {
    pub hash: ContentHash,
    pub items: Vec<(String, ContentHash)>,
}

impl MerkleRoot {
    pub fn compute(items: Vec<(String, ContentHash)>) -> Self {
        let hash = compute_root_hash(&items);
        Self { hash, items }
    }

    pub fn find_changed_items(
        old: &[(String, ContentHash)],
        new: &[(String, ContentHash)],
    ) -> Vec<(String, ItemChange)> {
        let old_map: HashMap<&str, ContentHash> = old.iter().map(|(n, h)| (n.as_str(), *h)).collect();
        let new_map: HashMap<&str, ContentHash> = new.iter().map(|(n, h)| (n.as_str(), *h)).collect();
        let mut changed = Vec::new();
        for (name, new_hash) in &new_map {
            match old_map.get(name) {
                Some(old_hash) if old_hash == new_hash => {}
                Some(_) => changed.push((name.to_string(), ItemChange::Modified)),
                None => changed.push((name.to_string(), ItemChange::Added)),
            }
        }
        for name in old_map.keys() {
            if !new_map.contains_key(name) {
                changed.push((name.to_string(), ItemChange::Removed));
            }
        }
        changed
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ItemChange {
    Modified,
    Added,
    Removed,
}

pub fn compute_root_hash(items: &[(String, ContentHash)]) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(b"merkle_root:");
    hasher.update((items.len() as u64).to_le_bytes());
    for (name, hash) in items {
        hasher.update((name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
        hasher.update(hash.as_bytes());
    }
    let digest = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&digest);
    ContentHash::from_bytes(bytes)
}
