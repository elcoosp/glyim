struct Iter<T> {
    data: *mut T,
    len: i64,
    pos: i64,
}

impl<T> Iter<T> {
    pub fn new(data: *mut T, len: i64) -> Iter<T> {
        Iter { data, len, pos: 0 }
    }

    pub fn next(mut self: Iter<T>) -> Option<T> {
        if self.pos >= self.len {
            None
        } else {
            let ptr = __ptr_offset(self.data as *mut u8, self.pos * __size_of::<T>()) as *mut T;
            let value = *ptr;
            self.pos = self.pos + 1;
            Some(value)
        }
    }
}
