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
        let h = key as i64;
        if h < 0 { 0 - h } else { h }
    }

    // Helper that inserts an entry without checking load factor again.
    fn raw_insert(mut self: HashMap<K, V>, key: K, value: V) -> HashMap<K, V> {
        let hash = self.hash(key);
        let mut idx = hash - (hash / self.cap) * self.cap;
        let mut done = 0;
        while done == 0 {
            match self.buckets.get(idx) {
                Some(entry) => {
                    if entry.occupied == 0 {
                        let new_entry = Entry { key, value, occupied: 1 };
                        self.buckets = self.buckets.set(idx, new_entry);
                        self.len = self.len + 1;
                        done = 1
                    } else {
                        if entry.key == key {
                            let new_entry = Entry { key, value, occupied: 1 };
                            self.buckets = self.buckets.set(idx, new_entry);
                            done = 1
                        } else {
                            idx = idx + 1;
                            if idx >= self.cap { idx = 0 }
                        }
                    }
                },
                None => { done = 1 }
            }
        };
        self
    }

    pub fn insert(self: HashMap<K, V>, key: K, value: V) -> HashMap<K, V> {
        if self.len * 10 >= self.cap * 7 || self.cap == 0 {
            return self.grow().raw_insert(key, value)
        };
        self.raw_insert(key, value)
    }

    pub fn get(self: HashMap<K, V>, key: K) -> Option<V> {
        if self.cap == 0 { return None; }
        let hash = self.hash(key);
        let mut idx = hash - (hash / self.cap) * self.cap;
        let mut count = 0;
        let mut found_val: V = 0 as V;
        let mut is_found = 0;
        let mut done = 0;
        while done == 0 {
            match self.buckets.get(idx) {
                Some(entry) => {
                    if entry.occupied == 0 {
                        done = 1
                    } else {
                        if entry.key == key {
                            found_val = entry.value;
                            is_found = 1;
                            done = 1
                        } else {
                            idx = idx + 1;
                            if idx >= self.cap { idx = 0; }
                            count = count + 1;
                            if count >= self.cap { done = 1 }
                        }
                    }
                },
                None => { done = 1 }
            }
        };
        if is_found != 0 { Some(found_val) } else { None }
    }

    pub fn len(self: HashMap<K, V>) -> i64 { self.len }

    fn grow(mut self: HashMap<K, V>) -> HashMap<K, V> {
        let old_buckets = self.buckets;
        let new_cap = if self.cap == 0 { 8 } else { self.cap * 2 };
        self.buckets = Vec::new();
        self.cap = new_cap;
        self.len = 0;
        // Fill with empty entries
        let mut i = 0;
        while i < new_cap {
            let empty = Entry { key: 0 as K, value: 0 as V, occupied: 0 };
            self.buckets = self.buckets.push(empty);
            i = i + 1
        };
        i = 0;
        while i < old_buckets.len() {
            match old_buckets.get(i) {
                Some(entry) => {
                    if entry.occupied != 0 {
                        // Use raw_insert to avoid triggering another grow
                        self = self.raw_insert(entry.key, entry.value)
                    }
                },
                None => {}
            };
            i = i + 1
        };
        self
    }
}
