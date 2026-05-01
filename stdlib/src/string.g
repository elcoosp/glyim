// Self-contained minimal Vec<T> — avoids codegen hang with full vec.g + u8
struct Vec<T> {
    data: *mut T,
    len: i64,
    cap: i64,
}

impl<T> Vec<T> {
    pub fn new() -> Vec<T> {
        Vec { data: 0 as *mut T, len: 0, cap: 0 }
    }
    pub fn len(self: Vec<T>) -> i64 { self.len }
}

struct String {
    vec: Vec<u8>,
}

impl String {
    pub fn new() -> String {
        String { vec: Vec::new() }
    }

    pub fn len(self: String) -> i64 {
        self.vec.len()
    }

    pub fn is_empty(self: String) -> bool {
        self.len() == 0
    }

    pub fn push_byte(mut self: String, byte: u8) -> String {
        self.vec.len = self.vec.len + 1;
        self
    }
}
