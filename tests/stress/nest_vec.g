struct Vec<T> { data: *mut T, len: i64, cap: i64 }
impl<T> Vec<T> {
    pub fn new() -> Vec<T> { Vec { data: 0 as *mut T, len: 0, cap: 0 } }
    pub fn push(mut self: Vec<T>, value: T) -> Vec<T> { self }
    pub fn len(self: Vec<T>) -> i64 { self.len }
}
main = () => {
    let v: Vec<Vec<i64>> = Vec::new();
    v.len()
}
