//! Typed index types and arena allocators.

use std::fmt;
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

/// Idx.
pub struct Idx<T> {
    raw: u32,
    _marker: PhantomData<T>,
}

impl<T> Clone for Idx<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Idx<T> {}
impl<T> PartialEq for Idx<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl<T> Eq for Idx<T> {}
impl<T> std::hash::Hash for Idx<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}
impl<T> PartialOrd for Idx<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Idx<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}
impl<T> fmt::Debug for Idx<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Idx({})", self.raw)
    }
}

impl<T> Idx<T> {
    #[inline]
    /// from_raw.
    pub fn from_raw(raw: u32) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }
    #[inline]
    /// to_raw.
    pub fn to_raw(self) -> u32 {
        self.raw
    }
    #[inline]
    /// index.
    pub fn index(self) -> usize {
        self.raw as usize
    }
}

/// IdxLike.
pub trait IdxLike: Copy + Eq + fmt::Debug + 'static {
    /// from_raw.
    fn from_raw(raw: u32) -> Self;
    /// to_raw.
    fn to_raw(self) -> u32;
    /// index.
    fn index(self) -> usize {
        self.to_raw() as usize
    }
}

impl<T: 'static> IdxLike for Idx<T> {
    fn from_raw(raw: u32) -> Self {
        Idx::from_raw(raw)
    }
    fn to_raw(self) -> u32 {
        self.to_raw()
    }
}

#[macro_export]
#[allow(missing_docs)]
macro_rules! define_idx {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[doc = "Newtype index into an arena-backed collection."]
        pub struct $name(u32);

        #[allow(missing_docs)]
        impl $name {
            #[inline]
            pub fn from_raw(raw: u32) -> Self {
                Self(raw)
            }
            #[inline]
            pub fn to_raw(self) -> u32 {
                self.0
            }
            #[inline]
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }

        #[allow(missing_docs)]
        impl $crate::arena::IdxLike for $name {
            fn from_raw(raw: u32) -> Self {
                Self(raw)
            }
            fn to_raw(self) -> u32 {
                self.0
            }
        }
    };
}

#[derive(Clone, Debug)]
/// IndexVec.
pub struct IndexVec<I: IdxLike, T> {
    raw: Vec<T>,
    _marker: PhantomData<I>,
}

impl<I: IdxLike, T> IndexVec<I, T> {
    /// new.
    pub fn new() -> Self {
        Self {
            raw: Vec::new(),
            _marker: PhantomData,
        }
    }
    /// with_capacity.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            raw: Vec::with_capacity(cap),
            _marker: PhantomData,
        }
    }
    /// from_raw.
    pub fn from_raw(raw: Vec<T>) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }
    /// push.
    pub fn push(&mut self, val: T) -> I {
        let idx = I::from_raw(self.raw.len() as u32);
        self.raw.push(val);
        idx
    }
    /// reserve.
    pub fn reserve(&mut self, additional: usize) {
        self.raw.reserve(additional);
    }
    /// len.
    pub fn len(&self) -> usize {
        self.raw.len()
    }
    /// is_empty.
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
    /// get.
    pub fn get(&self, idx: I) -> Option<&T> {
        debug_assert!(
            idx.index() < self.raw.len(),
            "IndexVec::get: index out of bounds"
        );
        self.raw.get(idx.index())
    }
    /// get_mut.
    pub fn get_mut(&mut self, idx: I) -> Option<&mut T> {
        self.raw.get_mut(idx.index())
    }
    /// iter.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.raw.iter()
    }
    /// iter_mut.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.raw.iter_mut()
    }
    /// iter_enumerated.
    pub fn iter_enumerated(&self) -> impl Iterator<Item = (I, &T)> {
        self.raw
            .iter()
            .enumerate()
            .map(|(i, v)| (I::from_raw(i as u32), v))
    }
    /// into_iter_enumerated.
    pub fn into_iter_enumerated(self) -> impl Iterator<Item = (I, T)> {
        self.raw
            .into_iter()
            .enumerate()
            .map(|(i, v)| (I::from_raw(i as u32), v))
    }
    /// into_raw.
    pub fn into_raw(self) -> Vec<T> {
        self.raw
    }
    /// as_slice.
    pub fn as_slice(&self) -> &[T] {
        &self.raw
    }
    /// as_mut_slice.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.raw
    }
    /// last.
    pub fn last(&self) -> Option<&T> {
        self.raw.last()
    }
}

impl<I: IdxLike, T> Default for IndexVec<I, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: IdxLike, T> Index<I> for IndexVec<I, T> {
    type Output = T;
    fn index(&self, idx: I) -> &T {
        &self.raw[idx.index()]
    }
}

impl<I: IdxLike, T> IndexMut<I> for IndexVec<I, T> {
    fn index_mut(&mut self, idx: I) -> &mut T {
        &mut self.raw[idx.index()]
    }
}
