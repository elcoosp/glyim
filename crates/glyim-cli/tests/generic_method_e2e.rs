use glyim_cli::pipeline;

#[test]
fn generic_method_len() {
    let src = r#"
struct Vec<T> { data: *mut T, len: i64, cap: i64 }
impl<T> Vec<T> {
    fn len(self: Vec<T>) -> i64 { self.len }
}
fn main() -> i64 {
    let v: Vec<i64> = Vec { data: 0 as *mut i64, len: 42, cap: 0 };
    v.len()
}
"#;
    let result = pipeline::run_jit(src).unwrap();
    assert_eq!(result, 42);
}

#[test]
fn generic_method_push() {
    let src = r#"
struct Vec<T> { data: *mut T, len: i64, cap: i64 }
impl<T> Vec<T> {
    fn push(mut self: Vec<T>, value: T) -> Vec<T> {
        self.len = self.len + 1;
        self
    }
}
fn main() -> i64 {
    let v: Vec<i64> = Vec { data: 0 as *mut i64, len: 0, cap: 0 };
    let v = v.push(10);
    v.len
}
"#;
    let result = pipeline::run_jit(src).unwrap();
    assert_eq!(result, 1);
}

#[test]
fn generic_method_get() {
    let src = r#"
struct Vec<T> { data: *mut T, len: i64, cap: i64 }
impl<T> Vec<T> {
    fn get(self: Vec<T>, index: i64) -> Option<T> {
        if index >= self.len { None } else { Some(*( (self.data as *mut u8 + index * 8) as *mut T)) }
    }
}
fn main() -> i64 {
    let v: Vec<i64> = Vec { data: 0 as *mut i64, len: 0, cap: 0 };
    match v.get(0) {
        Some(x) => x,
        None => -1,
    }
}
"#;
    let result = pipeline::run_jit(src).unwrap();
    assert_eq!(result, -1);
}

#[test]
fn generic_method_hashmap_insert_get() {
    let src = r#"
struct Entry<K, V> { key: K, value: V, occupied: i64 }
struct HashMap<K, V> { buckets: Vec<Entry<K,V>>, len: i64, cap: i64 }
impl<K, V> HashMap<K, V> {
    fn new() -> HashMap<K, V> {
        HashMap { buckets: Vec { data: 0 as *mut Entry<K,V>, len: 0, cap: 0 }, len: 0, cap: 0 }
    }
    fn insert(mut self: HashMap<K, V>, key: K, value: V) -> HashMap<K, V> {
        self.len = self.len + 1;
        self
    }
    fn len(self: HashMap<K, V>) -> i64 { self.len }
}
fn main() -> i64 {
    let m: HashMap<i64, i64> = HashMap::new();
    let m = m.insert(1, 100);
    m.len()
}
"#;
    let result = pipeline::run_jit(src).unwrap();
    assert_eq!(result, 1);
}
