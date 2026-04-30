struct VecI64 {
    data: *mut i64,
    len: i64,
    cap: i64,
}

impl VecI64 {
    pub fn new() -> VecI64 {
        VecI64 { data: 0 as *mut i64, len: 0, cap: 0 }
    }

    pub fn push(mut self: VecI64, value: i64) {
        if self.len == self.cap {
            let new_cap = if self.cap == 0 { 8 } else { self.cap * 2 };
            let new_data: *mut i64 = glyim_alloc(new_cap * 8) as *mut i64;
            if self.data != (0 as *mut i64) {
                let i = 0;
                while i < self.len {
                    let src_ptr = self.data + i;
                    let dst_ptr = new_data + i;
                    *dst_ptr = *src_ptr;
                    i = i + 1
                };
                glyim_free(self.data as *mut i64)
            };
            self.data = new_data;
            self.cap = new_cap
        };
        let dst = self.data + self.len;
        *dst = value;
        self.len = self.len + 1
    }

    pub fn get(self: VecI64, index: i64) -> i64 {
        if index >= self.len { 0 } else { *(self.data + index) }
    }

    pub fn len(self: VecI64) -> i64 { self.len }
}
