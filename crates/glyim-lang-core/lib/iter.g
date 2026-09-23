//! Iterator traits and adapters for the Glyim core library.

/// A trait for dealing with iterators.
trait Iterator {
    /// The type of the elements being iterated over.
    type Item;

    /// Advances the iterator and returns the next value.
    fn next(&mut self) -> Option<Self::Item>;

    /// Returns the bounds on the remaining length of the iterator.
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }

    /// Returns the number of elements in the iterator.
    fn count(self) -> usize {
        let mut count = 0;
        let mut this = self;
        while this.next().is_some() {
            count += 1;
        }
        count
    }

    /// Returns the last element of the iterator.
    fn last(self) -> Option<Self::Item> {
        let mut last = Option::None;
        let mut this = self;
        while let Option::Some(val) = this.next() {
            last = Option::Some(val);
        }
        last
    }

    /// Returns the `n`th element of the iterator.
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        for _ in 0..n {
            if self.next().is_none() {
                return Option::None;
            }
        }
        self.next()
    }

    /// Creates an iterator which gives the current iteration count as well as the next value.
    fn enumerate(self) -> Enumerate<Self> {
        Enumerate { iter: self, count: 0 }
    }

    /// Creates an iterator that yields elements based on a predicate.
    fn filter(self, predicate: fn(&Self::Item) -> bool) -> Filter<Self> {
        Filter { iter: self, predicate }
    }

    /// Creates an iterator that maps each element using a closure.
    fn map<B>(self, f: fn(Self::Item) -> B) -> Map<Self, B> {
        Map { iter: self, f }
    }

    /// Consumes the iterator, returning the remaining elements as a collection.
    fn collect<B: FromIterator<Self::Item>>(self) -> B {
        B::from_iter(self)
    }

    /// Creates an iterator that skips the first `n` elements.
    fn skip(self, n: usize) -> Skip<Self> {
        Skip { iter: self, n }
    }

    /// Creates an iterator that yields the first `n` elements.
    fn take(self, n: usize) -> Take<Self> {
        Take { iter: self, n }
    }

    /// Folds every element into an accumulator by applying an operation, returning the final result.
    fn fold<B>(self, init: B, f: fn(B, Self::Item) -> B) -> B {
        let mut acc = init;
        let mut this = self;
        while let Option::Some(item) = this.next() {
            acc = f(acc, item);
        }
        acc
    }

    /// Checks if all elements match a predicate.
    fn all(self, f: fn(Self::Item) -> bool) -> bool {
        let mut this = self;
        while let Option::Some(item) = this.next() {
            if !f(item) {
                return false;
            }
        }
        true
    }

    /// Checks if any elements match a predicate.
    fn any(self, f: fn(Self::Item) -> bool) -> bool {
        let mut this = self;
        while let Option::Some(item) = this.next() {
            if f(item) {
                return true;
            }
        }
        false
    }

    /// Searches for an element of an iterator that satisfies a predicate.
    fn find(&mut self, predicate: fn(&Self::Item) -> bool) -> Option<Self::Item> {
        while let Option::Some(item) = self.next() {
            if predicate(&item) {
                return Option::Some(item);
            }
        }
        Option::None
    }

    /// Calls a closure on each element of an iterator.
    fn for_each(self, f: fn(Self::Item)) {
        let mut this = self;
        while let Option::Some(item) = this.next() {
            f(item);
        }
    }

    /// Returns the minimum element of an iterator.
    fn min(self) -> Option<Self::Item> where Self::Item: Ord {
        let mut result = self.next()?;
        let mut this = self;
        while let Option::Some(item) = this.next() {
            if item < result {
                result = item;
            }
        }
        Option::Some(result)
    }

    /// Returns the maximum element of an iterator.
    fn max(self) -> Option<Self::Item> where Self::Item: Ord {
        let mut result = self.next()?;
        let mut this = self;
        while let Option::Some(item) = this.next() {
            if item > result {
                result = item;
            }
        }
        Option::Some(result)
    }
}

/// A trait for iterators that can be iterated from both ends.
trait DoubleEndedIterator: Iterator {
    /// Removes and returns an element from the end of the iterator.
    fn next_back(&mut self) -> Option<Self::Item>;
}

/// A trait for creating iterators from values.
trait IntoIterator {
    /// The type of the elements being iterated over.
    type Item;
    /// Which kind of iterator are we turning this into?
    type IntoIter: Iterator<Item = Self::Item>;

    /// Creates an iterator from a value.
    fn into_iter(self) -> Self::IntoIter;
}

/// Conversion from an `Iterator`.
trait FromIterator<A> {
    /// Creates a value from an iterator.
    fn from_iter<T: Iterator<Item = A>>(iter: T) -> Self;
}

/// Extension trait for `Iterator`.
trait ExactSizeIterator: Iterator {
    /// Returns the exact remaining length of the iterator.
    fn len(&self) -> usize {
        let (lower, upper) = self.size_hint();
        upper.expect("ExactSizeIterator must have an upper bound")
    }

    /// Returns `true` if the iterator is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// === Iterator Adapters ===

/// An iterator that yields the current count and element during iteration.
struct Enumerate<I: Iterator> {
    iter: I,
    count: usize,
}

impl<I: Iterator> Iterator for Enumerate<I> {
    type Item = (usize, I::Item);

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Option::Some(val) => {
                let idx = self.count;
                self.count += 1;
                Option::Some((idx, val))
            }
            Option::None => Option::None,
        }
    }
}

/// An iterator that filters elements using a predicate.
struct Filter<I: Iterator> {
    iter: I,
    predicate: fn(&I::Item) -> bool,
}

impl<I: Iterator> Iterator for Filter<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        while let Option::Some(item) = self.iter.next() {
            if (self.predicate)(&item) {
                return Option::Some(item);
            }
        }
        Option::None
    }
}

/// An iterator that maps each element using a closure.
struct Map<I: Iterator, B> {
    iter: I,
    f: fn(I::Item) -> B,
}

impl<B, I: Iterator> Iterator for Map<I, B> {
    type Item = B;

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Option::Some(item) => Option::Some((self.f)(item)),
            Option::None => Option::None,
        }
    }
}

/// An iterator that skips over `n` elements.
struct Skip<I: Iterator> {
    iter: I,
    n: usize,
}

impl<I: Iterator> Iterator for Skip<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        while self.n > 0 {
            self.n -= 1;
            self.iter.next();
        }
        self.iter.next()
    }
}

/// An iterator that only iterates over the first `n` iterations.
struct Take<I: Iterator> {
    iter: I,
    n: usize,
}

impl<I: Iterator> Iterator for Take<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.n > 0 {
            self.n -= 1;
            self.iter.next()
        } else {
            Option::None
        }
    }
}

/// An empty iterator.
struct Empty;

impl<T> Iterator for Empty {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> { Option::None }
    fn size_hint(&self) -> (usize, Option<usize>) { (0, Some(0)) }
}

/// Creates an iterator that yields nothing.
fn empty<T>() -> Empty { Empty }

/// An iterator that yields exactly one element.
struct Once<T> {
    val: Option<T>,
}

impl<T> Iterator for Once<T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> { self.val.take() }
}

/// Creates an iterator that yields exactly one element.
fn once<T>(val: T) -> Once<T> { Once { val: Option::Some(val) } }

/// An iterator that repeats an element endlessly.
struct Repeat<T> {
    val: T,
}

impl<T: Clone> Iterator for Repeat<T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> { Option::Some(self.val.clone()) }
}

/// Creates an iterator that endlessly repeats a single element.
fn repeat<T: Clone>(val: T) -> Repeat<T> { Repeat { val } }

/// IntoIterator for slice references.
impl<'a, T> IntoIterator for &'a [T] {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Iter<'a, T> {
        Iter { slice: self }
    }
}

/// An iterator that iterates over a slice by reference.
struct Iter<'a, T> {
    slice: &'a [T],
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.slice.is_empty() {
            Option::None
        } else {
            let (first, rest) = self.slice.split_first().unwrap();
            self.slice = rest;
            Option::Some(first)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.slice.len(), Some(self.slice.len()))
    }
}

/// IntoIterator for Range<usize>
struct RangeIter {
    range: Range<usize>,
}

impl Iterator for RangeIter {
    type Item = usize;
    fn next(&mut self) -> Option<Self::Item> {
        if self.range.start < self.range.end {
            let v = self.range.start;
            self.range.start += 1;
            Option::Some(v)
        } else {
            Option::None
        }
    }
}

impl IntoIterator for Range<usize> {
    type Item = usize;
    type IntoIter = RangeIter;
    fn into_iter(self) -> RangeIter { RangeIter { range: self } }
}
