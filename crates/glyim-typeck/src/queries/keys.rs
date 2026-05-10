use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// A fingerprint (content hash) for query keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub u64);

impl Fingerprint {
    pub fn of(data: &[u8]) -> Self {
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        Fingerprint(hasher.finish())
    }

    pub fn of_string(s: &str) -> Self {
        Self::of(s.as_bytes())
    }

    pub fn combine(&self, other: &Fingerprint) -> Fingerprint {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        other.0.hash(&mut hasher);
        Fingerprint(hasher.finish())
    }
}

/// A query key for type checking results.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum QueryKey {
    ItemType(glyim_hir::types::ExprId),
    TraitImpl {
        trait_name: String,
        type_name: String,
    },
    MacroExpansion {
        macro_name: String,
        input_hash: Fingerprint,
    },
    ReflectMeta(String),
}
