struct Entry<K, V> {
    key: K,
    value: V,
    occupied: i64,
}

struct HashMap<K, V> {
    buckets: Vec<Entry<K, V>>,
    len: i64,
    cap: i64,
}

impl<K, V> HashMap<K, V> {
    pub fn new() -> HashMap<K, V> {
        HashMap { buckets: Vec::new(), len: 0, cap: 0 }
    }

    fn hash(self: HashMap<K, V>, key: K) -> i64 {
        let seed = glyim_hash_seed();
        let bytes = 0 as *const u8;
        let hash = glyim_hash_bytes(bytes, __size_of::<K>());
        if hash < 0 { 0 - hash } else { hash }
    }

    pub fn insert(mut self: HashMap<K, V>, key: K, value: V) -> HashMap<K, V> {
        self.len = self.len + 1;
        self
    }

    pub fn get(self: HashMap<K, V>, key: K) -> Option<V> {
        if self.cap == 0 { return None; }
        let hash = self.hash(key);
        let mut idx = hash - (hash / self.cap) * self.cap;
        let mut count = 0;
        loop {
            let entry = self.buckets.get(idx);
            if entry.occupied == 0 { return None; }
            if entry.key == key { return Some(entry.value); }
            idx = idx + 1; if idx >= self.cap { idx = 0; }
            count = count + 1;
            if count >= self.cap { return None; }
        };
        None
    }

    pub fn len(self: HashMap<K, V>) -> i64 { self.len }
}
