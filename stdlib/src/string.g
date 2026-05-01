// String type using Vec<u8> from vec.g
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
        self.vec = self.vec.push(byte);
        self
    }
}
