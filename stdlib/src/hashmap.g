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
        if self.len * 10 >= self.cap * 7 || self.cap == 0 {
            self = self.grow()
        };
        let hash = self.hash(key);
        let mut idx = hash - (hash / self.cap) * self.cap;
        loop {
            let entry = self.buckets.get(idx);
            if entry.occupied == 0 {
                let new_entry = Entry { key, value, occupied: 1 };
                self.buckets = self.buckets.set(idx, new_entry);
                self.len = self.len + 1;
                return self
            };
            if entry.key == key {
                let new_entry = Entry { key, value, occupied: 1 };
                self.buckets = self.buckets.set(idx, new_entry);
                return self
            };
            idx = idx + 1;
            if idx >= self.cap { idx = 0 }
        };
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
            idx = idx + 1;
            if idx >= self.cap { idx = 0; }
            count = count + 1;
            if count >= self.cap { return None; }
        };
        None
    }

    pub fn len(self: HashMap<K, V>) -> i64 { self.len }

    fn grow(mut self: HashMap<K, V>) -> HashMap<K, V> {
        let old_buckets = self.buckets;
        let new_cap = if self.cap == 0 { 8 } else { self.cap * 2 };
        self.buckets = Vec::new();
        self.cap = new_cap;
        self.len = 0;
        let mut i = 0;
        while i < new_cap {
            let empty = Entry { key: 0 as K, value: 0 as V, occupied: 0 };
            self.buckets = self.buckets.push(empty);
            i = i + 1
        };
        let mut i = 0;
        while i < old_buckets.len() {
            let entry = old_buckets.get(i);
            if entry.occupied != 0 {
                self = self.insert(entry.key, entry.value)
            };
            i = i + 1
        };
        self
    }
}
