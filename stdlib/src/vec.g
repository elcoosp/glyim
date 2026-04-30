// A growable array type.

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
        if self.len == self.cap {
            let new_cap = if self.cap == 0 { 8 } else { self.cap * 2 };
            let size = __size_of::<T>();
            let new_data = glyim_alloc(new_cap * size) as *mut u8;
            if self.data != (0 as *mut u8) {
                let i = 0;
                while i < self.len {
                    let src = __ptr_offset(self.data, i * size) as *mut T;
                    let dst = __ptr_offset(new_data, i * size) as *mut T;
                    *dst = *src;
                    i = i + 1
                };
                glyim_free(self.data)
            };
            self.data = new_data;
            self.cap = new_cap
        };
        let dst = __ptr_offset(self.data, self.len * __size_of::<T>()) as *mut T;
        *dst = value;
        self.len = self.len + 1
    }

    pub fn get(&self, index: i64) -> Option<T> {
        if index >= self.len {
            None
        } else {
            Some(*(__ptr_offset(self.data, index * __size_of::<T>()) as *mut T))
        }
    }

    pub fn len(&self) -> i64 {
        self.len
    }
}
