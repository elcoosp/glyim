//! String slice operations for the Glyim core library.
//!
//! T140 [STD-15]: `len` / `contains` are implemented here. `trim`,
//! `Chars::next`, and `Lines::next` remain compiler-intrinsic stubs — they
//! need either native `&str` sub-slicing (`&s[a..b]` currently rejects a
//! dynamic range on `str`) or unicode decoding, neither of which the
//! language exposes yet.

/// Extension methods for string slices.
impl str {
    /// Returns the length of `self`.
    ///
    /// T140: delegates to `as_bytes().len()`. A `str` is byte-indexable
    /// (`&self[a..b]` yields a `&str`), and `as_bytes()` gives the byte slice
    /// whose `len` is the byte length — identical to the builtin's field
    /// extraction.
    fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// Returns `true` if `self` has a length of zero bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if `needle` occurs anywhere in `self`.
    ///
    /// T140: implemented as a naive byte-window search over `as_bytes()`
    /// (both operands). Correct for byte-substring matching (the common use:
    /// searching for ASCII delimiters like `\n`, `" "`).
    fn contains(&self, needle: &str) -> bool {
        let h = self.as_bytes();
        let n = needle.as_bytes();
        let hn = h.len();
        let nn = n.len();
        if nn == 0 {
            return true;
        }
        if nn > hn {
            return false;
        }
        let mut i = 0;
        while i + nn <= hn {
            let mut j = 0;
            let mut ok = true;
            while j < nn {
                if h[i + j] != n[j] {
                    ok = false;
                }
                j = j + 1;
            }
            if ok {
                return true;
            }
            i = i + 1;
        }
        false
    }

    /// Returns `true` if `self` begins with `prefix`.
    ///
    /// Byte-scan over `as_bytes()`; a prefix longer than the receiver
    /// is trivially `false`.
    fn starts_with(&self, prefix: &str) -> bool {
        let h = self.as_bytes();
        let p = prefix.as_bytes();
        let hn = h.len();
        let pn = p.len();
        if pn > hn {
            return false;
        }
        let mut i = 0;
        while i < pn {
            if h[i] != p[i] {
                return false;
            }
            i = i + 1;
        }
        true
    }

    /// Returns `true` if `self` ends with `suffix`.
    ///
    /// Byte-scan from the end; a suffix longer than the receiver is
    /// trivially `false`.
    fn ends_with(&self, suffix: &str) -> bool {
        let h = self.as_bytes();
        let s = suffix.as_bytes();
        let hn = h.len();
        let sn = s.len();
        if sn > hn {
            return false;
        }
        let off = hn - sn;
        let mut i = 0;
        while i < sn {
            if h[off + i] != s[i] {
                return false;
            }
            i = i + 1;
        }
        true
    }

    /// Returns an iterator over the chars of the string slice.
    fn chars(&self) -> Chars {
        Chars { s: self }
    }

    /// Returns an iterator over the lines of the string.
    fn lines(&self) -> Lines {
        Lines { s: self }
    }

    /// Returns a string slice with leading and trailing whitespace removed.
    ///
    /// T140: byte-scan + `&self[start..end]` sub-slicing. Strips the six
    /// ASCII whitespace bytes: space, tab, LF, CR, form-feed, vertical tab.
    /// Non-ASCII whitespace is not stripped.
    fn trim(&self) -> &str {
        let b = self.as_bytes();
        let n = b.len();
        let mut start = 0;
        while start < n {
            let c = b[start];
            if c == 32 || c == 9 || c == 10 || c == 13 || c == 12 || c == 11 {
                start = start + 1;
            } else {
                break;
            }
        }
        let mut end = n;
        while end > start {
            let c = b[end - 1];
            if c == 32 || c == 9 || c == 10 || c == 13 || c == 12 || c == 11 {
                end = end - 1;
            } else {
                break;
            }
        }
        &self[start..end]
    }

    /// Parses this string slice into another type.
    fn parse<T: FromStr>(&self) -> Result<T, T::Err> {
        T::from_str(self)
    }
}

/// An iterator over the chars of a string slice.
struct Chars<'a> {
    s: &'a str,
}

impl<'a> Iterator for Chars<'a> {
    type Item = char;
    fn next(&mut self) -> Option<char> {
        // compiler intrinsic: yields the next unicode scalar (needs decoding).
    }
}

/// An iterator over the lines of a string.
struct Lines<'a> {
    s: &'a str,
}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<&'a str> {
        // yields next line (split on \n, stripping \r)
    }
}

/// A trait for types that can be parsed from a string.
trait FromStr: Sized {
    /// The associated error which can be returned from parsing.
    type Err;

    /// Parses a string `s` to return a value of this type.
    fn from_str(s: &str) -> Result<Self, Self::Err>;
}
