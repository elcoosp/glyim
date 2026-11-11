//! Single source of truth for every trait that the compiler pre-registers at
//! a fixed, reserved `TraitDefId` before processing user source — the
//! trait-side mirror of [`crate::builtin_adts::BuiltinAdt`].
//!
//! TraitDefIds in the range `2001..3000` are **reserved for builtin traits**
//! and are disjoint from the builtin `AdtId` reservation (`1000..2000`,
//! see `BuiltinAdt::adt_id`) and from user-declared `LocalDefId`s (which
//! start at 0). Two consumers register this table:
//!
//! * `TyCtxMut::register_builtin_ranges` (resolver/typeck side), which
//!   drives [`BuiltinTraitDef::register_all`] through the
//!   [`BuiltinTraitSink`] impl on `TyCtxMut`;
//! * `glyim_pipeline::register_builtin_traits` (solver bootstrap), which
//!   drives the same table into the solver's `TraitContext`.
//!
//! Both used to hand-copy the table, and the copies had already drifted
//! once: `Default` (2016) and `Any` (2017) were registered in the type
//! context but never reached the solver, so solver-facing rendering saw an
//! unknown trait id for them. With a single registry, registering means
//! calling `register_all` — a consumer that forgets an entry fails the
//! parity tests (`builtin_trait_registry.rs` in `glyim-type`,
//! `builtin_trait_tests.rs` in `glyim-pipeline`) instead of silently
//! diverging.
//!
//! Adding a new builtin requires adding a variant here — nothing else. The
//! pipeline's exhaustive `BuiltinTraitDef` match over the structural-kind
//! mapping refuses to build until the new variant is classified, and
//! `ALL`/`from_name`/`from_trait_id` pick it up automatically.

/// Every trait pre-registered by the compiler at a reserved `TraitDefId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinTraitDef {
    /// `Future` — the async/await desugaring targets it.
    Future,
    /// `Drop` — drop glue consults it.
    Drop,
    /// `Deref` — deref coercion and method resolution consult it.
    Deref,
    /// `DerefMut` — mutable deref coercion.
    DerefMut,
    /// `Clone` — explicit duplication bound.
    Clone,
    /// `Copy` — implicit-duplication structural trait.
    Copy,
    /// `Sized` — metadata/stack-layout structural trait.
    Sized,
    /// `Send` — thread-transfer structural trait.
    Send,
    /// `Sync` — shared-reference structural trait.
    Sync,
    /// `Unpin` — move-out-of-pin structural trait.
    Unpin,
    /// `Fn` — call-operator trait family.
    Fn,
    /// `FnMut` — call-operator trait family.
    FnMut,
    /// `FnOnce` — call-operator trait family.
    FnOnce,
    /// `Iterator` — `for`-loop desugaring targets it.
    Iterator,
    /// `IntoIterator` — `for`-loop desugaring targets it.
    IntoIterator,
    /// `Default` — default-value construction; re-declared by the assembled
    /// stdlib (`default.g`) but not declared as a lang item.
    Default,
    /// `Any` — dynamic typing root (`dyn Any + Send` in `thread.g`'s
    /// `Result<T, Box<dyn Any + Send>>`). Unlike `Send`/`Sync` it is not
    /// declared in the assembled stdlib, so it must be a builtin trait for
    /// `dyn Any` to resolve.
    Any,
}

impl BuiltinTraitDef {
    /// Every variant, in reserved-id order — `register_all` visits entries
    /// in this order, and the parity tests pin the ids to be contiguous.
    pub const ALL: &'static [Self] = &[
        Self::Future,
        Self::Drop,
        Self::Deref,
        Self::DerefMut,
        Self::Clone,
        Self::Copy,
        Self::Sized,
        Self::Send,
        Self::Sync,
        Self::Unpin,
        Self::Fn,
        Self::FnMut,
        Self::FnOnce,
        Self::Iterator,
        Self::IntoIterator,
        Self::Default,
        Self::Any,
    ];

    /// The reserved `TraitDefId` (as a raw u32) for this builtin. Ids are
    /// published API: the resolver, the solver bootstrap, and diagnostics
    /// all hand these exact values around.
    #[must_use]
    pub fn trait_id(self) -> u32 {
        match self {
            Self::Future => 2001,
            Self::Drop => 2002,
            Self::Deref => 2003,
            Self::DerefMut => 2004,
            Self::Clone => 2005,
            Self::Copy => 2006,
            Self::Sized => 2007,
            Self::Send => 2008,
            Self::Sync => 2009,
            Self::Unpin => 2010,
            Self::Fn => 2011,
            Self::FnMut => 2012,
            Self::FnOnce => 2013,
            Self::Iterator => 2014,
            Self::IntoIterator => 2015,
            Self::Default => 2016,
            Self::Any => 2017,
        }
    }

    /// The source-level name this trait is registered under. Must match the
    /// identifier a `.g` source writes in a bound (`T: Clone`) or an impl
    /// header (`impl Drop for Y`).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Future => "Future",
            Self::Drop => "Drop",
            Self::Deref => "Deref",
            Self::DerefMut => "DerefMut",
            Self::Clone => "Clone",
            Self::Copy => "Copy",
            Self::Sized => "Sized",
            Self::Send => "Send",
            Self::Sync => "Sync",
            Self::Unpin => "Unpin",
            Self::Fn => "Fn",
            Self::FnMut => "FnMut",
            Self::FnOnce => "FnOnce",
            Self::Iterator => "Iterator",
            Self::IntoIterator => "IntoIterator",
            Self::Default => "Default",
            Self::Any => "Any",
        }
    }

    /// Whether the solver may discharge this trait's bounds structurally.
    /// Exactly the five auto/structural traits (`Copy`/`Sized`/`Send`/
    /// `Sync`/`Unpin`) qualify; `prove_trait` maps them onto the solver's
    /// `BuiltinTrait` kinds and every other entry is proven from `impl`s.
    #[must_use]
    pub fn is_lang_trait(self) -> bool {
        matches!(
            self,
            Self::Copy | Self::Sized | Self::Send | Self::Sync | Self::Unpin
        )
    }

    /// Look up a variant by its source-level name — the canonicalization
    /// path for a name that must land on the reserved id (`None` when the
    /// name is not a builtin trait).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.name() == name)
    }

    /// Look up a variant by its reserved trait id (`None` outside the
    /// builtin range, which includes every builtin `AdtId`).
    #[must_use]
    pub fn from_trait_id(trait_id: u32) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.trait_id() == trait_id)
    }

    /// Drive the whole table into `sink` in reserved-id order. This is the
    /// only registration path a consumer should use: hand-copying the table
    /// is what produced the resolver/solver drift this module replaces.
    pub fn register_all(sink: &mut impl BuiltinTraitSink) {
        for t in Self::ALL {
            sink.register_builtin_trait(t.trait_id(), t.name());
        }
    }
}

/// Registration target for [`BuiltinTraitDef::register_all`]. A sink turns
/// the registry's `(id, name)` pairs into whatever its context needs —
/// interning plus a `trait_by_name` insert on the resolver side, a
/// `TraitDef` row on the solver side — without the registry knowing about
/// either context's internals.
pub trait BuiltinTraitSink {
    /// Register one builtin trait under its reserved id and source-level
    /// name. `register_all` calls this exactly once per [`BuiltinTraitDef`]
    /// variant, in ascending id order. The name is `&'static str` because
    /// every registry name is a program-lifetime constant — sinks may store
    /// it without copying.
    fn register_builtin_trait(&mut self, trait_id: u32, name: &'static str);
}
