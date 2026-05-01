struct String {
    vec: Vec<u8>,
}

impl String {
    pub fn new() -> String {
        String { vec: Vec::new() }
    }

    pub fn from(s: &str) -> String {
        let v = Vec::new()
        let mut i = 0
        while i < s.len() {
            let ptr = __ptr_offset(s.data, i) as *mut u8
            v.push(*ptr)
            i = i + 1
        }
        String { vec: v }
    }

    pub fn len(&self) -> i64 {
        self.vec.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }
}
