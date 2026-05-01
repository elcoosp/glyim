struct Range {
    start: i64,
    end: i64,
    current: i64,
}

impl Range {
    pub fn new(start: i64, end: i64) -> Range {
        Range { start, end, current: start }
    }

    pub fn next(mut self: Range) -> Option<i64> {
        if self.current >= self.end {
            None
        } else {
            let val = self.current;
            self.current = self.current + 1;
            Some(val)
        }
    }
}
