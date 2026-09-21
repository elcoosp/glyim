//! Language-item registry: the single source of truth for builtin types and
//! traits (`Option`, `Range`, `String`, `Vec`, `Box`, `Drop`, …).
//!
//! Previously these were referenced by hardcoded `AdtId`/`DefId` constants
//! scattered across `glyim-lower`, `glyim-solve`, `glyim-pipeline`, and this
//! crate (see the de-stubbing plan §1.1). That was fragile: a builtin's id
//! could silently collide with a user ADT, and the same builtin was spelled
//! differently in different crates. Every consumer now queries this registry
//! instead of inventing a number.

use glyim_core::def_id::DefId;
use std::collections::HashMap;

/// The set of compiler-known items. Each variant names one builtin the
/// language/stdlib depends on. Add variants here as new builtins are required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LangItem {
    /// Variant.
    Option,
    /// Variant.
    Result,
    /// Variant.
    Range,
    /// Variant.
    RangeInclusive,
    /// Variant.
    RangeFrom,
    /// Variant.
    RangeTo,
    /// Variant.
    RangeToInclusive,
    /// Variant.
    RangeFull,
    /// Variant.
    String,
    /// Variant.
    Str,
    /// Variant.
    Vec,
    /// Variant.
    Box,
    /// Variant.
    Drop,
    /// Variant.
    Deref,
    /// Variant.
    DerefMut,
    /// Variant.
    Send,
    /// Variant.
    Sync,
    /// Variant.
    Copy,
    /// Variant.
    Clone,
    /// Variant.
    Iterator,
    /// Variant.
    IntoIterator,
    /// Variant.
    FnOnce,
    /// Variant.
    FnMut,
    /// Variant.
    Fn,
    /// Variant.
    Future,
    /// Variant.
    GlobalAlloc,
    /// Variant.
    Allocator,
}

/// Error returned when a lang-item registration or lookup fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LangItemError {
    /// Two different `DefId`s were registered for the same `LangItem`.
    Duplicate {
        /// Struct.
        item: LangItem,
        /// Struct.
        existing: DefId,
        /// Struct.
        new: DefId,
    },
    /// No `DefId` was registered for the requested `LangItem` (e.g. the core
    /// library is missing the `#[lang = "..."]` attribute, or crate loading
    /// forgot to register it).
    Missing(LangItem),
}

/// The registry itself: a bidirectional-ish map from `LangItem` to its
/// resolved `DefId`. Populated once during crate loading (builtins via
/// `register_builtin_ranges`, then stdlib items via `#[lang]` attributes as
/// that parser support lands).
#[derive(Default, Clone)]
pub struct LangItems {
    map: HashMap<LangItem, DefId>,
}

impl LangItems {
    /// Register `def_id` as the implementation of `item`.
    ///
    /// Returns `Err(LangItemError::Duplicate)` if `item` was already
    /// registered with a *different* `DefId`; re-registering the same `DefId`
    /// is idempotent and succeeds.
    pub fn register(&mut self, item: LangItem, def_id: DefId) -> Result<(), LangItemError> {
        match self.map.get(&item) {
            Some(&existing) if existing != def_id => {
                return Err(LangItemError::Duplicate {
                    item,
                    existing,
                    new: def_id,
                });
            }
            _ => {}
        }
        self.map.insert(item, def_id);
        Ok(())
    }

    /// Look up the `DefId` for `item`, if registered.
    pub fn get(&self, item: LangItem) -> Option<DefId> {
        self.map.get(&item).copied()
    }

    /// Look up the `DefId` for `item`, erroring if absent. Use this at the
    /// point a builtin is *required* — the error becomes a diagnosable
    /// configuration problem (e.g. "core library missing `#[lang = \"option\"]`")
    /// rather than a silent fallback to a bogus id.
    pub fn require(&self, item: LangItem) -> Result<DefId, LangItemError> {
        self.get(item).ok_or(LangItemError::Missing(item))
    }
}
