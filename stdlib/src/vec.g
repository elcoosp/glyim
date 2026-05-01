struct Vec<T> {
    data: *mut u8,
    len: i64,
    cap: i64,
}

impl<T> Vec<T> {
    pub fn new() -> Vec<T> {
        Vec { data: 0 as *mut u8, len: 0, cap: 0 }
    }

    pub fn len(&self) -> i64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

    pub fn push(&mut self, value: T) {
        let elem_size = __size_of::<T>()
        if self.len == self.cap {
            let new_cap = if self.cap == 0 { 8 } else { self.cap * 2 }
            let new_data = glyim_alloc(new_cap * elem_size)
            if self.data != (0 as *mut u8) {
                let mut i = 0
                while i < self.len {
                    let src = __ptr_offset(self.data, i * elem_size) as *mut T
                    let dst = __ptr_offset(new_data, i * elem_size) as *mut T
                    *dst = *src
                    i = i + 1
                }
                glyim_free(self.data)
            }
            self.data = new_data
            self.cap = new_cap
        }
        let dst = __ptr_offset(self.data, self.len * elem_size) as *mut T
        *dst = value
        self.len = self.len + 1
    }

    pub fn get(&self, index: i64) -> T {
        if index < 0 || index >= self.len {
            abort()
        }
        let elem_size = __size_of::<T>()
        let ptr = __ptr_offset(self.data, index * elem_size) as *mut T
        *ptr
    }

    pub fn pop(&mut self) -> T {
        if self.len == 0 {
            abort()
        }
        self.len = self.len - 1
        let elem_size = __size_of::<T>()
        let ptr = __ptr_offset(self.data, self.len * elem_size) as *mut T
        *ptr
    }
}
