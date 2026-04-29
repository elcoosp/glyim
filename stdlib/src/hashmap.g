// Glyim Standard Library — HashMap<K, V>
//
// A hash map using open addressing with linear probing and FNV-1a hash.
//
// STATUS: Cannot be compiled by glyim v0.5.1
// BLOCKERS:
//   §1. Generic *mut T pointer for bucket array not supported
//   §2. Pointer load/store not supported
//   §3. No way to hash bytes (need pointer to byte array)
//   §4. Entry comparison requires Eq trait dispatch
//   §5. Entry<K,V> struct layout needs pointer-sized fields
//
// DESIGN (for when blockers are resolved):
//   struct Entry<K, V> { key: K, value: V, occupied: bool }
//   struct HashMap<K, V> { buckets: *mut Entry<K, V>, len: i64, cap: i64 }
//
// fn HashMap::new<K, V>() -> HashMap<K, V> { ... }
// fn HashMap::insert<K, V>(self: *mut HashMap<K, V>, key: K, value: V) { ... }
// fn HashMap::get<K, V>(self: *HashMap<K, V>, key: K) -> Option<V> { ... }
// fn HashMap::remove<K, V>(self: *mut HashMap<K, V>, key: K) -> Option<V> { ... }
// fn HashMap::len<K, V>(self: *HashMap<K, V>) -> i64 { ... }
// fn HashMap::is_empty<K, V>(self: *HashMap<K, V>) -> bool { ... }
// fn HashMap::drop<K, V>(self: *mut HashMap<K, V>) { ... }
// fn HashMap::grow<K, V>(self: *mut HashMap<K, V>) { ... }
// fn HashMap::find_slot<K, V>(self: *HashMap<K, V>, key: K) -> i64 { ... }
