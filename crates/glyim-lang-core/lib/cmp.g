//! Comparison traits and functions for the Glyim core library.

/// Trait for equality comparisons which are partial equivalence relations.
trait PartialEq<Rhs = Self> {
    fn eq(&self, other: &Rhs) -> bool;
    fn ne(&self, other: &Rhs) -> bool {
        !self.eq(other)
    }
}

/// Trait for equality comparisons which are equivalence relations.
trait Eq: PartialEq {}

/// Trait for values that can be compared for a sort-order.
trait PartialOrd<Rhs = Self>: PartialEq<Rhs> {
    fn partial_cmp(&self, other: &Rhs) -> Option<Ordering>;

    fn lt(&self, other: &Rhs) -> bool {
        matches!(self.partial_cmp(other), Some(Ordering::Less))
    }

    fn le(&self, other: &Rhs) -> bool {
        matches!(self.partial_cmp(other), Some(Ordering::Less | Ordering::Equal))
    }

    fn gt(&self, other: &Rhs) -> bool {
        matches!(self.partial_cmp(other), Some(Ordering::Greater))
    }

    fn ge(&self, other: &Rhs) -> bool {
        matches!(self.partial_cmp(other), Some(Ordering::Greater | Ordering::Equal))
    }
}

/// Trait for types that form a total order.
trait Ord: Eq + PartialOrd {
    fn cmp(&self, other: &Self) -> Ordering;

    fn max(self, other: Self) -> Self {
        match self.cmp(&other) {
            Ordering::Less | Ordering::Equal => other,
            Ordering::Greater => self,
            // The memory-order variants are not part of the `Ord` total
            // order; they exist so atomic call sites (`Ordering::SeqCst`)
            // resolve. `cmp` never yields them, so these arms are
            // unreachable for well-typed code.
            Ordering::Acquire | Ordering::Release | Ordering::Relaxed | Ordering::SeqCst => self,
        }
    }

    fn min(self, other: Self) -> Self {
        match self.cmp(&other) {
            Ordering::Less | Ordering::Equal => self,
            Ordering::Greater => other,
            Ordering::Acquire | Ordering::Release | Ordering::Relaxed | Ordering::SeqCst => self,
        }
    }

    fn clamp(self, min: Self, max: Self) -> Self {
        if self.lt(&min) { min }
        else if self.gt(&max) { max }
        else { self }
    }
}

/// An ordering comparison.
enum Ordering {
    /// Less than.
    Less,
    /// Equal.
    Equal,
    /// Greater than.
    Greater,
    /// Acquire ordering (atomic).
    Acquire,
    /// Release ordering (atomic).
    Release,
    /// Relaxed ordering (atomic).
    Relaxed,
    /// Sequentially consistent ordering (atomic).
    SeqCst,
}

/// Compares and returns the minimum of two values.
fn min<T: Ord>(v1: T, v2: T) -> T {
    v1.min(v2)
}

/// Compares and returns the maximum of two values.
fn max<T: Ord>(v1: T, v2: T) -> T {
    v1.max(v2)
}

/// A helper struct for reverse ordering.
struct Reverse<T>(pub T);
