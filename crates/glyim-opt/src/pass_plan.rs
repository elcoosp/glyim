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

/// Dispatch seam for one MIR optimization pass.
///
/// A pass sees the (validated) body and the read-only type context; it
/// mutates only `body`. Passes must preserve the invariants
/// [`crate::validate_body`] checks — the pipeline re-validates nothing
/// after arbitrary custom plans beyond the `Subslice` post-condition that
/// codegen structurally requires (see [`crate::optimize_with_plan`]).
pub trait MirPass {
    /// Stable pass id — the string phase profiles and custom plans use to
    /// refer to this pass. Unique across [`BuiltinMirPass::ALL`].
    fn id(&self) -> &'static str;

    /// Run the pass over `body` in place.
    fn run(&self, ctx: &TyCtx, body: &mut Body);
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
