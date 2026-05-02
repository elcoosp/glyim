struct Vec<T> {
    data: *mut T,
    len: i64,
    cap: i64,
}

impl<T> Vec<T> {
    pub fn new() -> Vec<T> {
        Vec { data: 0 as *mut T, len: 0, cap: 0 }
    }

    pub fn push(mut self: Vec<T>, value: T) -> Vec<T> {
        if self.len == self.cap {
            let new_cap = if self.cap == 0 { 8 } else { self.cap * 2 };
            let elem_size = __size_of::<T>();
            let new_data: *mut T = glyim_alloc(new_cap * elem_size) as *mut T;
            if self.data != (0 as *mut T) {
                let mut i = 0;
                while i < self.len {
                    let src_ptr = __ptr_offset(self.data as *mut u8, i * elem_size) as *mut T;
                    let dst_ptr = __ptr_offset(new_data as *mut u8, i * elem_size) as *mut T;
                    *dst_ptr = *src_ptr;
                    i = i + 1
                };
                glyim_free(self.data as *mut u8)
            };
            self.data = new_data;
            self.cap = new_cap
        };
        let dst = __ptr_offset(self.data as *mut u8, self.len * __size_of::<T>()) as *mut T;
        *dst = value;
        self.len = self.len + 1;
        self
    }

    pub fn get(self: Vec<T>, index: i64) -> Option<T> {
        if index >= self.len {
            None
        } else {
            let elem_size = __size_of::<T>();
            let ptr = __ptr_offset(self.data as *mut u8, index * elem_size) as *mut T;
            Some(*ptr)
        }
    }
    pub fn set(mut self: Vec<T>, index: i64, value: T) -> Vec<T> {
        if index < 0 || index >= self.len {
            return self;
        };
        let elem_size = __size_of::<T>();
        let dst = __ptr_offset(self.data as *mut u8, index * elem_size) as *mut T;
        *dst = value;
        self
    }


    pub fn set(mut self: Vec<T>, index: i64, value: T) -> Vec<T> {
        if index >= self.len {
            self
        } else {
            let elem_size = __size_of::<T>();
            let dst = __ptr_offset(self.data as *mut u8, index * elem_size) as *mut T;
            *dst = value;
            self
        }
    }

    pub fn len(self: Vec<T>) -> i64 { self.len }

    pub fn pop(mut self: Vec<T>) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len = self.len - 1;
            let elem_size = __size_of::<T>();
            let ptr = __ptr_offset(self.data as *mut u8, self.len * elem_size) as *mut T;
            Some(*ptr)
        }
    }

    pub fn set(mut self: Vec<T>, index: i64, value: T) -> Vec<T> {
        if index >= self.len {
            self
        } else {
            let elem_size = __size_of::<T>();
            let dst = __ptr_offset(self.data as *mut u8, index * elem_size) as *mut T;
            *dst = value;
            self
        }
    }
}
