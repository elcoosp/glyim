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
            let new_data: *mut i64 = __glyim_alloc(new_cap * 8) as *mut i64;
            if self.data != (0 as *mut i64) {
                let mut i = 0;
                while i < self.len {
                    let src_ptr = __ptr_offset(self.data as *mut u8, i * 8) as *mut i64;
                    let dst_ptr = __ptr_offset(new_data as *mut u8, i * 8) as *mut i64;
                    *dst_ptr = *src_ptr;
                    i = i + 1
                };
                __glyim_free(self.data as *mut u8)
            };
            self.data = new_data;
            self.cap = new_cap
        };
        let dst = __ptr_offset(self.data as *mut u8, self.len * 8) as *mut i64;
        *dst = value;
        self.len = self.len + 1
    }

    pub fn get(self: VecI64, index: i64) -> i64 {
        if index >= self.len { 0 } else { *(__ptr_offset(self.data as *mut u8, index * 8) as *mut i64) }
    }

    pub fn len(self: VecI64) -> i64 { self.len }
}
