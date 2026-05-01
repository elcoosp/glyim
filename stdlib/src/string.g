struct String {
    vec: Vec<u8>,
}

impl String {
    pub fn new() -> String {
        String { vec: Vec::new() }
    }

    pub fn from_bytes(data: *const u8, len: i64) -> String {
        let mut v = Vec::new();
        let mut i = 0;
        while i < len {
            let ptr = __ptr_offset(data as *mut u8, i);
            let ch = *(ptr as *mut u8);
            v = v.push(ch);
            i = i + 1
        };
        String { vec: v }
    }

    pub fn len(self: String) -> i64 {
        self.vec.len()
    }

    pub fn is_empty(self: String) -> bool {
        self.len() == 0
    }

    pub fn push_byte(mut self: String, ch: u8) -> String {
        self.vec = self.vec.push(ch);
        self
    }

    pub fn push_str(mut self: String, s: &str) -> String {
        // &str is a fat pointer: { ptr: *const u8, len: i64 }
        // Access via pointer manipulation
        let ptr = s as *const u8;
        let len = 0; // placeholder - need proper &str field access
        // For now, push_str is limited - users should use from_bytes
        let mut i = 0;
        while i < len {
            let ch_ptr = __ptr_offset(ptr as *mut u8, i);
            let ch = *(ch_ptr as *mut u8);
            self.vec = self.vec.push(ch);
            i = i + 1
        };
        self
    }
}
