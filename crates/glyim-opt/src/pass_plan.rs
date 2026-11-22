//! Opt-level pass plans: the seam between "which MIR passes exist" and
//! "which passes a compile actually runs".
//!
//! Before this module, [`crate::optimize`] hard-wired one fixed pass
//! sequence that ran identically at every `--opt-level`: the CLI threaded
//! the level into the LLVM backend, the linker, and the compile-cache key
//! (so `-O0` and `-O2` cached separately) — but the MIR optimizer ignored
//! it, and `-O0` paid for const-prop, DCE, and CFG simplification it never
//! asked for. The `elaborate_drops` entry point (the sixth pass) had no
//! production caller at all.
//!
//! This module makes the pass set a registry, following the same pattern
//! as `BuiltinAdt`/`BuiltinTraitDef` in `glyim-type`:
//!
//! * [`MirPass`] is the dispatch seam — one `run` per pass, uniform
//!   `&TyCtx` receiver (drop elaboration's `&mut TyCtxMut` signature was
//!   an accident: it only reads the ctx and interns types);
//! * [`BuiltinMirPass`] is the single source of truth for the pass set and
//!   its stable ids (`pass.id()`), with `from_id` canonicalization;
//! * [`PassPlan`] is an ordered selection, produced either explicitly
//!   ([`PassPlan::from_passes`]) or from an opt level
//!   ([`PassPlan::for_opt_level`]).
//!
//! Level semantics (matching rustc's `mir-opt-level` behavior):
//!
//! * `-O0` runs only `slice_desugar` — the one pass codegen structurally
//!   requires (both backends assume `Subslice` projections are gone, see
//!   `slice_desugar`'s module doc). Everything else is semantics-preserving
//!   but optional, so `-O0` keeps the raw lowering shape (which also makes
//!   `--emit=mir` output closer to what lowering produced).
//! * `-O1`..`-O3` (and any larger level, saturating) run the full
//!   historical pipeline in the exact order [`crate::optimize`] has always
//!   used, so optimized builds are unchanged bit-for-bit in pass sequence.
//!
//! `drop_elaboration` is registered but deliberately absent from every
//! default level plan: running it changes terminator shapes (drop flags
//! materialize as locals and `SetDiscriminant`-style writes), and the
//! partial-move story it models is a registered ledger gap
//! (`ledger/gaps/known`). Plans that need it select it explicitly via
//! [`PassPlan::from_passes`] — it is a first-class pass, not dead API.

use crate::Body;
use glyim_type::TyCtx;

/// A named MIR invariant a pass declares as a precondition (`REQUIRES`) or a
/// postcondition (`ENSURES`). The pass manager (M5.11) checks the always-on
/// `WellFormed` set via [`glyim_mir::validate::validate_body`] after every
/// pass; a pass that establishes a phase-specific post-condition (only
/// `slice_desugar` → `NoSubslice` today) declares it via [`MirPass::ensures`]
/// so the manager runs the matching validator arm
/// ([`glyim_mir::validate::validate_no_subslice`]) right after it — and after
/// every subsequent pass, so a pass that *reintroduces* a `Subslice` is
/// attributed precisely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invariant {
    /// The body satisfies every always-on invariant
    /// (`validate_body`: CFG edges, declaredness, storage pairing, arg
    /// count, cleanup edges, drop glue, slice-projection terminality).
    WellFormed,
    /// No `ProjectionElem::Subslice` survives anywhere in the body (the
    /// `slice_desugar` post-condition, audit MIR-31).
    NoSubslice,
}

/// Per-pass deterministic statistics (M5.11). Every field is a structural
/// count, never a wall-clock reading, so a run is reproducible across
/// machines and the `--emit=stats` rows are deterministic. The manager
/// records one entry per pass in the plan (passes beyond a bisect limit are
/// `ran: false` with zero deltas).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassStats {
    /// The pass's stable id ([`MirPass::id`]).
    pub name: &'static str,
    /// Whether the pass ran (`false` under `optimize_with_bisect` beyond the
    /// limit).
    pub ran: bool,
    /// Whether the pass changed the body (the structural signature
    /// `(block_count, total_statements)` differs before vs after).
    pub body_changed: bool,
    /// Change in total statement count across all blocks (`after - before`).
    pub stmt_delta: i64,
    /// Change in basic-block count (`after - before`).
    pub block_delta: i64,
}

/// Dispatch seam for one MIR optimization pass.
///
/// A pass sees the (validated) body and the read-only type context; it
/// mutates only `body`. The manager enforces the contract: it runs
/// [`glyim_mir::validate::validate_body`] after every pass (the `WellFormed`
/// `ENSURES` every pass must maintain) and attributes any violation to the
/// pass that produced it ([`crate::PassViolation`]). A pass additionally
/// declares its phase-specific post-conditions via [`MirPass::ensures`] (e.g.
/// `slice_desugar` ensures `NoSubslice`); the manager runs the matching
/// validator arm after that pass and after every subsequent pass.
pub trait MirPass {
    /// Stable pass id — the string phase profiles and custom plans use to
    /// refer to this pass. Unique across [`BuiltinMirPass::ALL`].
    fn id(&self) -> &'static str;

    /// Run the pass over `body` in place.
    fn run(&self, ctx: &TyCtx, body: &mut Body);

    /// Invariants the pass requires of its input (`REQUIRES`). Every builtin
    /// pass requires a `WellFormed` body; the manager guarantees this by
    /// validating up front and after every prior pass, so a pass never sees
    /// an ill-formed input. Returning a narrower set than `WellFormed` is a
    /// documented relaxation (none of the builtins do).
    fn requires(&self) -> &'static [Invariant] {
        &[Invariant::WellFormed]
    }

    /// Phase-specific post-conditions the pass establishes (`ENSURES`), beyond
    /// the always-on `WellFormed` the manager checks after every pass. The
    /// manager runs the matching validator arm after this pass and after
    /// every subsequent pass in the plan, so a later pass that breaks the
    /// post-condition is attributed, not the pass that established it.
    fn ensures(&self) -> &'static [Invariant] {
        &[]
    }
}

/// Every MIR pass the optimizer ships, in registry order.
///
/// Registry order is documentation order — plans define their own run
/// order; [`PassPlan::for_opt_level`] pins the historical sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinMirPass {
    /// Slice/subslice projection desugaring. Required for codegen: both
    /// backends treat a surviving `Subslice` projection as
    /// `unreachable!()`, so every codegen-facing plan includes it.
    SliceDesugar,
    /// Constant folding of `Operand::Constant`/`Rvalue` shapes.
    ConstantProp,
    /// Dead-store and dead-block elimination.
    Dce,
    /// CFG simplification (single-predecessor chains, empty blocks).
    CfgSimplify,
    /// Unreachable-block elimination.
    UnreachableElim,
    /// Drop-flag elaboration for conditional drops. Registered for
    /// explicit plans only — see the module doc for why no default level
    /// enables it.
    DropElaboration,
}

impl MirPass for BuiltinMirPass {
    fn id(&self) -> &'static str {
        match self {
            Self::SliceDesugar => "slice_desugar",
            Self::ConstantProp => "constant_prop",
            Self::Dce => "dce",
            Self::CfgSimplify => "cfg_simplify",
            Self::UnreachableElim => "unreachable_elim",
            Self::DropElaboration => "drop_elaboration",
        }
    }

    fn run(&self, ctx: &TyCtx, body: &mut Body) {
        match self {
            Self::SliceDesugar => crate::slice_desugar::run(ctx, body),
            Self::ConstantProp => crate::constant_prop::run(ctx, body),
            Self::Dce => crate::dce::run(ctx, body),
            Self::CfgSimplify => crate::cfg_simplify::run(ctx, body),
            Self::UnreachableElim => crate::unreachable_elim::run(ctx, body),
            Self::DropElaboration => crate::drop_elaboration::run(ctx, body),
        }
    }

    /// `slice_desugar` is the one pass that establishes a phase-specific
    /// post-condition: once it has run, no `Subslice` projection may survive
    /// anywhere in the body (audit MIR-31). The other builtins preserve
    /// `WellFormed` only, which the manager checks after every pass.
    fn ensures(&self) -> &'static [Invariant] {
        match self {
            Self::SliceDesugar => &[Invariant::NoSubslice],
            Self::ConstantProp
            | Self::Dce
            | Self::CfgSimplify
            | Self::UnreachableElim
            | Self::DropElaboration => &[],
        }
    }
}

impl BuiltinMirPass {
    /// Every variant — the fixed surface `from_id` round-trips over.
    pub const ALL: &'static [Self] = &[
        Self::SliceDesugar,
        Self::ConstantProp,
        Self::Dce,
        Self::CfgSimplify,
        Self::UnreachableElim,
        Self::DropElaboration,
    ];

    /// Look up a variant by its stable id (`None` for unknown ids — the
    /// caller decides whether that is an error, the registry never
    /// guesses).
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|p| p.id() == id)
    }
}

/// An ordered, duplicate-free selection of passes.
///
/// Built with [`PassPlan::from_passes`] (explicit) or
/// [`PassPlan::for_opt_level`] (the CLI-facing default). The order is the
/// run order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassPlan {
    passes: Vec<BuiltinMirPass>,
}

impl PassPlan {
    /// Build a plan from an explicit, ordered pass list.
    ///
    /// Returns `None` when the list contains a duplicate — a plan is a set
    /// with an order, and silently running a pass twice would hide the
    /// ambiguity. An empty list is legal (an identity plan for analysis
    /// callers), but codegen-facing plans must include
    /// [`BuiltinMirPass::SliceDesugar`] — both backends assume its
    /// elimination of `Subslice` projections.
    #[must_use]
    pub fn from_passes(passes: &[BuiltinMirPass]) -> Option<Self> {
        let mut seen = std::collections::HashSet::new();
        let mut ordered = Vec::with_capacity(passes.len());
        for p in passes {
            if !seen.insert(*p) {
                return None;
            }
            ordered.push(*p);
        }
        Some(Self { passes: ordered })
    }

    /// The plan for a CLI `--opt-level` value.
    ///
    /// * `0` — minimal: only [`BuiltinMirPass::SliceDesugar`] (the
    ///   codegen-required pass). No DCE, no const-prop: the body keeps the
    ///   shape lowering produced.
    /// * `1..=3` and anything larger — the full historical pipeline in the
    ///   order [`crate::optimize`] has always run.
    ///
    /// `slice_desugar` is present at every level: codegen structurally
    /// requires it, so no opt level may drop it.
    #[must_use]
    pub fn for_opt_level(opt_level: u8) -> Self {
        if opt_level == 0 {
            Self {
                passes: vec![BuiltinMirPass::SliceDesugar],
            }
        } else {
            Self {
                passes: vec![
                    BuiltinMirPass::SliceDesugar,
                    BuiltinMirPass::ConstantProp,
                    BuiltinMirPass::Dce,
                    BuiltinMirPass::CfgSimplify,
                    BuiltinMirPass::UnreachableElim,
                ],
            }
        }
    }

    /// The passes, in run order.
    #[must_use]
    pub fn passes(&self) -> &[BuiltinMirPass] {
        &self.passes
    }

    /// The pass ids, in run order — the per-pass observability rows print
    /// exactly these strings.
    #[must_use]
    pub fn ids(&self) -> Vec<&'static str> {
        self.passes.iter().map(|p| p.id()).collect()
    }

    /// Whether the plan runs `pass`.
    #[must_use]
    pub fn includes(&self, pass: BuiltinMirPass) -> bool {
        self.passes.contains(&pass)
    }
}
