pub mod mph;

/// Struct-of-Arrays reflection metadata for a type.
#[derive(Clone, Debug)]
pub struct TypeMetaSoA {
    pub type_id: u32,
    pub type_name: String,
    pub field_count: u32,
    pub name_hashes: Vec<u64>,
    pub offsets: Vec<usize>,
    pub type_ids: Vec<u32>,
    pub getters: Vec<usize>,
    pub mph_seed: u64,
}

pub fn generate_type_meta(
    type_id: u32,
    type_name: &str,
    field_names: &[String],
    field_types: &[u32],
) -> TypeMetaSoA {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    let name_hashes: Vec<u64> = field_names.iter().map(|name| {
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        hasher.finish()
    }).collect();

    TypeMetaSoA {
        type_id,
        type_name: type_name.to_string(),
        field_count: field_names.len() as u32,
        name_hashes,
        offsets: vec![0; field_names.len()],
        type_ids: field_types.to_vec(),
        getters: vec![0; field_names.len()],
        mph_seed: 0,
    }
}
