//! Single source of truth for every ADT that the compiler pre-registers at
//! a fixed, reserved `AdtId` before processing user source.
//!
//! AdtIds in the range `1000..2000` are **reserved for builtins** and are
//! disjoint from user-declared `LocalDefId`s (which start at 0). The
//! `register_builtin_ranges` function in `ty_ctx_mut.rs` must register every
//! variant of this enum using `BuiltinAdt::X.adt_id()`, and any code that
//! needs to canonicalize a user-declared alias of a builtin name to its
//! reserved id must go through `BuiltinAdt::from_name(...).adt_id()`.
//!
//! Adding a new builtin requires adding a variant here — nothing else.
//! The compiler will refuse to build if a match on `BuiltinAdt` is not
//! exhaustive, so no variant can be silently forgotten.

/// Every ADT pre-registered by `register_builtin_ranges`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinAdt {
    // Range types — the compiler treats these specially (slicing, `for`,
    // range patterns all lower through `lang_items::Range*`).
    Range,
    RangeInclusive,
    RangeFrom,
    RangeTo,
    RangeToInclusive,

    // Interior mutability — the borrow checker consults `Cell`/`RefCell`/
    // `UnsafeCell` to relax aliasing rules.
    UnsafeCell,

    // Fundamental library types that the standard library ALSO declares in
    // `.g` source. User-declared `enum Option<T>` must resolve to the same
    // `AdtId` as the builtin placeholder, or the same logical type splits
    // into two incompatible AdtIds.
    Option,
    Result,
    Ordering,
    ExitStatus,
    Vec,
    PhantomData,
    Box,
    String,
}

impl BuiltinAdt {
    /// Every variant, for use by `register_builtin_ranges`'s iteration.
    pub const ALL: &'static [Self] = &[
        Self::Range,
        Self::RangeInclusive,
        Self::RangeFrom,
        Self::RangeTo,
        Self::RangeToInclusive,
        Self::UnsafeCell,
        Self::Option,
        Self::Result,
        Self::Ordering,
        Self::ExitStatus,
        Self::Vec,
        Self::PhantomData,
        Self::Box,
        Self::String,
    ];

    /// The reserved `AdtId` (as a raw u32) for this builtin.
    pub fn adt_id(self) -> u32 {
        match self {
            Self::Range => 1000,
            Self::RangeInclusive => 1001,
            Self::RangeFrom => 1002,
            Self::RangeTo => 1003,
            Self::RangeToInclusive => 1004,
            Self::UnsafeCell => 1005,
            Self::Option => 1010,
            Self::Result => 1011,
            Self::Ordering => 1015,
            Self::Vec => 1020,
            Self::ExitStatus => 1022,
            Self::PhantomData => 1030,
            Self::Box => 1040,
            Self::String => 1050,
        }
    }

    /// The source-level name this builtin is registered under. Must match
    /// the identifier a `.g` source uses when it re-declares the type.
    pub fn name(self) -> &'static str {
        match self {
            Self::Range => "Range",
            Self::RangeInclusive => "RangeInclusive",
            Self::RangeFrom => "RangeFrom",
            Self::RangeTo => "RangeTo",
            Self::RangeToInclusive => "RangeToInclusive",
            Self::UnsafeCell => "UnsafeCell",
            Self::Option => "Option",
            Self::Result => "Result",
            Self::Ordering => "Ordering",
            Self::ExitStatus => "ExitStatus",
            Self::Vec => "Vec",
            Self::PhantomData => "PhantomData",
            Self::Box => "Box",
            Self::String => "String",
        }
    }

    /// Look up a variant by its source-level name.
    ///
    /// `Err(name)` — nothing in `BuiltinAdt` has that name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|b| b.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_ids_are_unique_and_in_reserved_range() {
        let mut seen = HashSet::new();
        for b in BuiltinAdt::ALL {
            let id = b.adt_id();
            assert!(
                (1000..2000).contains(&id),
                "{} has AdtId {id} outside the reserved 1000..2000 range",
                b.name(),
            );
            assert!(
                seen.insert(id),
                "duplicate AdtId {id} (from {})",
                b.name(),
            );
        }
    }

    #[test]
    fn all_names_are_unique_and_round_trip() {
        let mut seen = HashSet::new();
        for b in BuiltinAdt::ALL {
            let n = b.name();
            assert!(seen.insert(n), "duplicate name {n}");
            assert_eq!(BuiltinAdt::from_name(n), Some(*b), "round-trip failed for {n}");
        }
        assert_eq!(BuiltinAdt::from_name("NotAThing"), None);
    }
}
