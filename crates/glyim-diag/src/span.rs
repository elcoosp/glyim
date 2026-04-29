//! Source text span — a half-open byte range `[start, end)`.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        assert!(start <= end, "start must be <= end");
        Self { start, end }
    }
    pub fn len(&self) -> usize {
        self.end - self.start
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_valid_span() {
        let s = Span::new(5, 10);
        assert_eq!(s.start, 5);
        assert_eq!(s.end, 10);
    }
    #[test]
    fn new_zero_length_span() {
        let s = Span::new(3, 3);
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }
    #[test]
    fn len_returns_byte_count() {
        assert_eq!(Span::new(2, 7).len(), 5);
    }
    #[test]
    #[should_panic(expected = "start must be <= end")]
    fn new_panics_if_start_gt_end() {
        Span::new(10, 5);
    }
    #[test]
    fn span_is_copy() {
        let a = Span::new(0, 1);
        let _b = a;
        let _c = a;
    }
}
