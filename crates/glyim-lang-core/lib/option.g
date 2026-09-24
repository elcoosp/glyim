//! The `Option` type for the Glyim core library.
//!
//! `Option<T>` represents an optional value: every `Option` is either
//! `Some` and contains a value, or `None`, and does not.

use cmp::Ordering;

/// The `Option` type. See the module-level documentation for more.
enum Option<T> {
    /// No value.
    None,
    /// Some value of type `T`.
    Some(T),
}

impl<T> Option<T> {
    /// Returns `true` if the option is a `Some` value.
    fn is_some(&self) -> bool {
        match self {
            Option::Some(_) => true,
            Option::None => false,
        }
    }

    /// Returns `true` if the option is a `None` value.
    fn is_none(&self) -> bool {
        match self {
            Option::Some(_) => false,
            Option::None => true,
        }
    }

    /// Unwraps an option, yielding the content of a `Some`.
    ///
    /// Panics if the value is a `None` with a custom panic message.
    fn expect(self, msg: &str) -> T {
        match self {
            Option::Some(val) => val,
            Option::None => panic!("{}", msg),
        }
    }

    /// Unwraps an option, yielding the content of a `Some`.
    ///
    /// Panics if the value is a `None`.
    fn unwrap(self) -> T {
        match self {
            Option::Some(val) => val,
            Option::None => panic!("called `Option::unwrap()` on a `None` value"),
        }
    }

    /// Unwraps an option, returning the content of a `Some`, or a provided default.
    fn unwrap_or(self, default: T) -> T {
        match self {
            Option::Some(val) => val,
            Option::None => default,
        }
    }

    /// Returns the contained value or computes it from a closure.
    fn unwrap_or_else(self, f: fn() -> T) -> T {
        match self {
            Option::Some(val) => val,
            Option::None => f(),
        }
    }

    /// Returns the contained value or a default.
    fn unwrap_or_default(self) -> T where T: Default {
        match self {
            Option::Some(val) => val,
            Option::None => T::default(),
        }
    }

    /// Maps an `Option<T>` to `Option<U>` by applying a function to a contained value.
    fn map<U>(self, f: fn(T) -> U) -> Option<U> {
        match self {
            Option::Some(val) => Option::Some(f(val)),
            Option::None => Option::None,
        }
    }

    /// Returns the provided default result (if `None`), or applies a function to the
    /// contained value (if `Some`).
    fn map_or<U>(self, default: U, f: fn(T) -> U) -> U {
        match self {
            Option::Some(val) => f(val),
            Option::None => default,
        }
    }

    /// Transforms the `Option<T>` into a `Result<T, E>`, mapping `Some(v)` to `Ok(v)`
    /// and `None` to `Err(err)`.
    fn ok_or<E>(self, err: E) -> Result<T, E> {
        match self {
            Option::Some(val) => Result::Ok(val),
            Option::None => Result::Err(err),
        }
    }

    /// Returns `None` if the option is `None`, otherwise returns `optb`.
    fn and<U>(self, optb: Option<U>) -> Option<U> {
        match self {
            Option::Some(_) => optb,
            Option::None => Option::None,
        }
    }

    /// Returns `None` if the option is `None`, otherwise calls `f` with the wrapped
    /// value and returns the result.
    fn and_then<U>(self, f: fn(T) -> Option<U>) -> Option<U> {
        match self {
            Option::Some(val) => f(val),
            Option::None => Option::None,
        }
    }

    /// Returns the option if it contains a value, otherwise returns `optb`.
    fn or(self, optb: Option<T>) -> Option<T> {
        match self {
            Option::Some(_) => self,
            Option::None => optb,
        }
    }

    /// Returns the option if it contains a value, otherwise calls `f` and returns the result.
    fn or_else(self, f: fn() -> Option<T>) -> Option<T> {
        match self {
            Option::Some(_) => self,
            Option::None => f(),
        }
    }

    /// Converts from `&Option<T>` to `Option<&T>`.
    fn as_ref(&self) -> Option<&T> {
        match self {
            Option::Some(val) => Option::Some(val),
            Option::None => Option::None,
        }
    }

    /// Converts from `&mut Option<T>` to `Option<&mut T>`.
    fn as_mut(&mut self) -> Option<&mut T> {
        match self {
            Option::Some(val) => Option::Some(val),
            Option::None => Option::None,
        }
    }

    /// Inserts `value` into the option, then returns a mutable reference to it.
    fn insert(&mut self, value: T) -> &mut T {
        *self = Option::Some(value);
        match self {
            Option::Some(val) => val,
            Option::None => unreachable!(),
        }
    }

    /// Inserts a value into the option if it is `None`, then returns a mutable
    /// reference to the contained value.
    fn get_or_insert(&mut self, value: T) -> &mut T {
        if self.is_none() {
            *self = Option::Some(value);
        }
        match self {
            Option::Some(val) => val,
            Option::None => unreachable!(),
        }
    }

    /// Takes the value out of the option, leaving a `None` in its place.
    fn take(&mut self) -> Option<T> {
        mem::replace(self, Option::None)
    }

    /// Replaces the actual value in the option by the value given in parameter,
    /// returning the old value if present, leaving a `Some` in its place.
    fn replace(&mut self, value: T) -> Option<T> {
        mem::replace(self, Option::Some(value))
    }

    /// Zips `self` with another `Option`.
    fn zip<U>(self, other: Option<U>) -> Option<(T, U)> {
        match (self, other) {
            (Option::Some(a), Option::Some(b)) => Option::Some((a, b)),
            _ => Option::None,
        }
    }

    /// Returns a copy of the `Option` if it contains a `Copy` value.
    fn copied(self) -> Option<T> where T: Copy {
        self
    }

    /// Returns a clone of the `Option` if it contains a `Clone` value.
    fn cloned(self) -> Option<T> where T: Clone {
        self
    }
}

impl<T> Option<Option<T>> {
    /// Converts from `Option<Option<T>>` to `Option<T>`.
    fn flatten(self) -> Option<T> {
        self.and_then(|x| x)
    }
}

impl<T> From<T> for Option<T> {
    fn from(val: T) -> Option<T> {
        Option::Some(val)
    }
}

impl<T> Iterator for Option<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.take()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Option::Some(_) => (1, Some(1)),
            Option::None => (0, Some(0)),
        }
    }
}
