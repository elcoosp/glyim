struct String {
    vec: Vec<u8>,
}

impl String {
    pub fn new() -> String {
        String { vec: Vec::new() }
    }

    pub fn len(&self) -> i64 {
        self.vec.len()
    }
}
