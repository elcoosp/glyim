struct Vec<T> {
    data: *mut u8,
    len: i64,
    cap: i64,
}

impl<T> Vec<T> {
    pub fn new() -> Vec<T> {
        Vec { data: 0 as *mut u8, len: 0, cap: 0 }
    }

    pub fn push(&mut self, value: T) {
        let elem_size = 8;
        if self.len == self.cap {
            let new_cap = if self.cap == 0 { 8 } else { self.cap * 2 };
            let new_data = glyim_alloc(new_cap * elem_size) as *mut u8;
            if self.data != (0 as *mut u8) {
                let i = 0;
                while i < self.len {
                    let src = __ptr_offset(self.data, i * elem_size) as *mut i64;
                    let dst = __ptr_offset(new_data, i * elem_size) as *mut i64;
                    *dst = *src;
                    i = i + 1;
                };
                glyim_free(self.data);
            };
            self.data = new_data;
            self.cap = new_cap;
        };
        let dst = __ptr_offset(self.data, self.len * elem_size) as *mut i64;
        *dst = value;
        self.len = self.len + 1;
    }

    pub fn get(&self, index: i64) -> i64 {
        if index >= self.len { 0 }
        else { *(__ptr_offset(self.data, index * 8) as *mut i64) }
    }

    pub fn len(&self) -> i64 {
        self.len
    }
}
