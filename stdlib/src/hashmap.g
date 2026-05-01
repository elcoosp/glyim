struct Vec<T> { data: *mut T, len: i64, cap: i64 }
impl<T> Vec<T> {
    pub fn new() -> Vec<T> { Vec { data: 0 as *mut T, len: 0, cap: 0 } }
    pub fn len(self: Vec<T>) -> i64 { self.len }
}

struct Entry<K, V> {
    key: K,
    value: V,
    occupied: bool,
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

    pub fn insert(mut self: HashMap<K, V>, key: K, value: V) -> HashMap<K, V> {
        self.len = self.len + 1;
        self
    }

    pub fn get(self: HashMap<K, V>, key: K) -> Option<V> {
        None
    }

    pub fn len(self: HashMap<K, V>) -> i64 { self.len }
}
