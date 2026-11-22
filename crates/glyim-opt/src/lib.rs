//! MIR optimizations (constant propagation, dead code elimination, etc.)
//!
//! # Validator + pass manager (M5.5 / M5.11)
//!
//! The single MIR validator lives in [`glyim_mir::validate`]: it returns
//! `Result<(), Vec<Violation>>` and never panics. This crate re-exports it
//! ([`validate_body`], [`validate_no_subslice`], [`Violation`]) so historical
//! call sites that named `glyim_opt::validate_body` keep compiling — the
//! duplicate `glyim-opt/src/validate.rs` that predated the unification is
//! gone.
//!
//! [`optimize_with_plan`] is the pass manager entry point. It validates the
//! body up front, then runs each pass in the plan, re-validating after
//! **every** pass and attributing any violation to the pass that produced it
//! ([`PassViolation`]). Per-pass deterministic stats
//! ([`PassStats`]: name, ran, body-changed, statement/block deltas — no
//! wall-clock) are returned by [`optimize_with_bisect`], the bisect API the
//! `--opt-bisect-limit` flag will drive from the CLI.
// Stylistic clippy lints suppressed crate-wide (test-noise lints).

use glyim_mir::Body;
use glyim_type::TyCtx;
use std::sync::Arc;

mod cfg_simplify;
mod constant_prop;
mod dce;
mod slice_desugar;
mod unreachable_elim;

pub mod pass_plan;

pub use pass_plan::{BuiltinMirPass, Invariant, MirPass, PassPlan, PassStats};

/// The result of a successful [`optimize`] run: the same body with every
/// enabled MIR pass applied.
#[derive(Clone, Debug)]
pub struct Optimized {
    /// The optimized MIR body (a private clone of the input; the caller's
    /// `Arc<Body>` is never mutated).
    pub body: Body,
}

/// A pass-attributed batch of validator violations (M5.11). The pass manager
/// runs [`validate_body`] after every pass; when a pass produces an
/// ill-formed body, the violations it introduced are gathered here with the
/// offending pass's stable id, so the diagnostic names the pass rather than
/// reporting "optimization failed" three stages later in codegen.
#[derive(Debug)]
pub struct PassViolation {
    /// The stable id ([`MirPass::id`]) of the pass that produced the violating
    /// body.
    pub pass: &'static str,
    /// The violations the validator found in the body immediately after that
    /// pass ran.
    pub violations: Vec<Violation>,
}

/// The error a MIR optimization run surfaces (M5.10: MIR passes are
/// `Result`-returning; the optimizer never panics on an ill-formed body).
#[derive(Debug)]
pub enum OptError {
    /// The input body failed [`validate_body`] before any pass ran. These are
    /// the violations the lowering or a prior pipeline stage produced.
    InputInvalid(Vec<Violation>),
    /// A pass produced a body that fails the always-on validator. The
    /// offending pass is named in the [`PassViolation`].
    PassViolated(PassViolation),
    /// A pass's declared post-condition ([`MirPass::ensures`]) failed — today
    /// only `NoSubslice` (a `Subslice` projection survived `slice_desugar` or
    /// was reintroduced by a later pass). The offending pass is named.
    PostConditionViolated(PassViolation),
    /// A `Subslice` projection survived a plan that did not run
    /// `slice_desugar`. Codegen (both backends) treats a surviving `Subslice`
    /// as a compiler bug; this turns that would-be codegen ICE into a precise
    /// error here, attributed to the plan rather than to any single pass.
    SubsliceSurvived(Vec<Violation>),
}

impl std::fmt::Display for OptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputInvalid(v) => {
                write!(f, "MIR optimization: input body failed validation ({v:?})")
            }
            Self::PassViolated(pv) => {
                write!(
                    f,
                    "MIR optimization: pass `{}` produced an invalid body ({:?})",
                    pv.pass, pv.violations
                )
            }
            Self::PostConditionViolated(pv) => {
                write!(
                    f,
                    "MIR optimization: pass `{}` broke its declared post-condition ({:?})",
                    pv.pass, pv.violations
                )
            }
            Self::SubsliceSurvived(v) => {
                write!(
                    f,
                    "MIR optimization: Subslice projection survived a plan without slice_desugar ({v:?})"
                )
            }
        }
    }
}

impl std::error::Error for OptError {}

/// Run the full MIR optimization pipeline over `body`.
///
/// This is the back-compat entry point: it is exactly
/// [`optimize_with_plan`] with the plan [`PassPlan::for_opt_level`] picks
/// for an optimized build, i.e. the pass sequence this crate has always
/// run (`slice_desugar`, `constant_prop`, `dce`, `cfg_simplify`,
/// `unreachable_elim`). Compiles that honor the caller's `--opt-level`
/// go through [`optimize_at_level`] instead.
///
/// # Errors
///
/// Returns `Err` when the input body fails [`validate_body`], when a pass
/// produces an ill-formed body, or when a `Subslice` projection survives the
/// plan — all are compiler bugs surfaced as an [`OptError`] rather than an
/// ICE (M5.10 panic-ratchet: MIR passes are `Result`-returning).
pub fn optimize(ctx: &TyCtx, body: &Arc<Body>) -> Result<Optimized, OptError> {
    optimize_with_plan(ctx, body, &PassPlan::for_opt_level(2))
}

/// Run `plan` over `body`: validate up front, then run each pass in plan
/// order, re-validating after **every** pass (M5.11 verify-after-each-pass)
/// and attributing any violation to the pass that produced it. After a pass
/// that declares [`MirPass::ensures`] = `NoSubslice` (`slice_desugar`), and
/// after every subsequent pass, the `Subslice` post-condition is
/// re-checked too, so a pass that reintroduces a `Subslice` is named
/// precisely. A final `Subslice` check catches plans that omit
/// `slice_desugar` entirely.
///
/// The validation contract is level-independent by construction: the body is
/// validated *before* the first pass and *after* every pass, so a release
/// build catches any pass that produces an ill-formed body at the pass
/// boundary, not three stages later in codegen.
///
/// # Errors
///
/// See [`optimize`].
pub fn optimize_with_plan(
    ctx: &TyCtx,
    body: &Arc<Body>,
    plan: &PassPlan,
) -> Result<Optimized, OptError> {
    let mut body = (**body).clone();
    let passes: Vec<&dyn MirPass> = plan.passes().iter().map(|p| p as &dyn MirPass).collect();
    run_plan_with_verification(ctx, &mut body, &passes, None)?;
    Ok(Optimized { body })
}

/// Run the plan [`PassPlan::for_opt_level`] selects for `opt_level` over
/// `body`.
///
/// `-O0` runs only `slice_desugar` (the pass codegen structurally
/// requires); `-O1`..`-O3` and anything larger run the full pipeline
/// [`optimize`] runs. See [`PassPlan::for_opt_level`] for the level
/// semantics and [`optimize_with_plan`] for the errors.
pub fn optimize_at_level(
    ctx: &TyCtx,
    body: &Arc<Body>,
    opt_level: u8,
) -> Result<Optimized, OptError> {
    optimize_with_plan(ctx, body, &PassPlan::for_opt_level(opt_level))
}

/// Run `plan` over `body` but stop after the first `limit` passes (M5.11
/// `--opt-bisect-limit`). Returns the partially-optimized body and one
/// [`PassStats`] entry per pass in the plan (passes beyond `limit` are
/// `ran: false`).
///
/// This is the bisect API: a caller binary-searches `limit` in
/// `0..=plan.passes().len()` to find the first pass whose output fails
/// validation — the smallest `limit` at which the run returns `Err` names
/// the offending pass index (`limit - 1`). The manager still validates
/// after every pass that runs, so a corrupting pass is attributed via
/// [`OptError::PassViolated`] with the exact pass id. Unlike
/// [`optimize_with_plan`], this entry does *not* run the final
/// `Subslice`-survived safety net: a partial pipeline may legitimately
/// leave a pre-`slice_desugar` `Subslice` in place, and flagging it would
/// mask the corrupting pass the bisect is hunting.
///
/// # Errors
///
/// Returns [`OptError::InputInvalid`] when the input body fails validation,
/// or [`OptError::PassViolated`] / [`OptError::PostConditionViolated`] when a
/// pass that ran produced an ill-formed body.
pub fn optimize_with_bisect(
    ctx: &TyCtx,
    body: &Arc<Body>,
    plan: &PassPlan,
    limit: usize,
) -> Result<(Optimized, Vec<PassStats>), OptError> {
    let mut body = (**body).clone();
    let passes: Vec<&dyn MirPass> = plan.passes().iter().map(|p| p as &dyn MirPass).collect();
    let stats = run_plan_with_verification(ctx, &mut body, &passes, Some(limit))?;
    Ok((Optimized { body }, stats))
}

/// The shared pass-manager core: validate up front, run each pass with
/// verify-after-each-pass, and (when `limit` is `None`) run the final
/// `Subslice`-survived safety net. When `limit` is `Some(n)`, only the first
/// `n` passes run and per-pass [`PassStats`] are collected; otherwise no
/// stats are collected (the back-compat [`optimize_with_plan`] path does not
/// pay for them).
///
/// Taking `&[&dyn MirPass]` (not `&PassPlan`) lets the in-crate bisect test
/// inject a corrupting pass at a known index and prove the manager names it.
pub(crate) fn run_plan_with_verification(
    ctx: &TyCtx,
    body: &mut Body,
    passes: &[&dyn MirPass],
    limit: Option<usize>,
) -> Result<Vec<PassStats>, OptError> {
    validate_body(ctx, body).map_err(OptError::InputInvalid)?;
    let mut stats: Vec<PassStats> = if limit.is_some() {
        Vec::with_capacity(passes.len())
    } else {
        Vec::new()
    };
    let mut no_subslice_in_force = false;
    for (i, pass) in passes.iter().enumerate() {
        if let Some(n) = limit
            && i >= n
        {
            if limit.is_some() {
                stats.push(PassStats {
                    name: pass.id(),
                    ran: false,
                    body_changed: false,
                    stmt_delta: 0,
                    block_delta: 0,
                });
            }
            continue;
        }
        let before = body_signature(body);
        pass.run(ctx, body);
        // Verify-after-each-pass (M5.11): the always-on invariants must hold
        // immediately after the pass. Attribute failures to this pass.
        if let Err(violations) = validate_body(ctx, body) {
            return Err(OptError::PassViolated(PassViolation {
                pass: pass.id(),
                violations,
            }));
        }
        // Phase-specific post-conditions. `slice_desugar` establishes
        // `NoSubslice`; once it has run, every subsequent pass must preserve
        // it (so a reintroduction is attributed to the reintroducing pass).
        let establishes_no_subslice = pass.ensures().contains(&Invariant::NoSubslice);
        if establishes_no_subslice {
            no_subslice_in_force = true;
        }
        if establishes_no_subslice || no_subslice_in_force {
            if let Err(violations) = validate_no_subslice(body) {
                return Err(OptError::PostConditionViolated(PassViolation {
                    pass: pass.id(),
                    violations,
                }));
            }
        }
        if limit.is_some() {
            let after = body_signature(body);
            stats.push(PassStats {
                name: pass.id(),
                ran: true,
                body_changed: before != after,
                stmt_delta: after.1 - before.1,
                block_delta: after.0 - before.0,
            });
        }
    }
    // Safety net: a plan that did not run `slice_desugar` (so
    // `no_subslice_in_force` stayed false) but leaves a `Subslice` projection
    // would ICE codegen. Turn that into a precise error here. Skipped under
    // bisect (see `optimize_with_bisect`).
    if limit.is_none()
        && !no_subslice_in_force
        && let Err(violations) = validate_no_subslice(body)
    {
        return Err(OptError::SubsliceSurvived(violations));
    }
    Ok(stats)
}

/// A deterministic structural signature of `body` for [`PassStats`]:
/// `(basic-block count, total statement count)`. Two signatures differ iff
/// the pass changed the block count or the statement count — the cheapest
/// no-wall-clock change detector that still catches DCE/const-prop/CFG
/// simplification effects (each of which moves one of these counts).
fn body_signature(body: &Body) -> (i64, i64) {
    let blocks = body.basic_blocks.len() as i64;
    let stmts = body
        .basic_blocks
        .iter()
        .map(|b| b.statements.len() as i64)
        .sum();
    (blocks, stmts)
}

#[cfg(test)]
mod tests;

/// Drop elaboration (de-stubbing plan §8.2). When a `Drop` terminator's
/// operand is definitely initialized, it is replaced with a direct `Drop`;
/// otherwise it is lowered to a `Goto`. Also handles `Drop` on projected
/// places (deref/index/field) by dropping through them rather than skipping.
mod drop_elaboration;

/// Run drop elaboration on the MIR body.
///
/// Also mounted in the pass registry as [`BuiltinMirPass::DropElaboration`]
/// so explicit [`PassPlan`]s can select it; no default opt level includes
/// it (see the `pass_plan` module doc). Takes the same read-only `&TyCtx`
/// receiver every other pass takes — it only queries types and interns
/// the flag-array types it materializes.
pub fn elaborate_drops(ctx: &TyCtx, body: &mut Body) {
    drop_elaboration::run(ctx, body);
}

/// Validate MIR well-formedness (M5.5): the unified validator in
/// [`glyim_mir::validate`], re-exported so call sites that named
/// `glyim_opt::validate_body` keep compiling. Returns every violation found
/// (in deterministic block/statement order), or `Ok(())` if the body is
/// well-formed. Wired as the up-front + after-every-pass check inside
/// [`optimize_with_plan`] (M5.11 verify-after-each-pass).
pub use glyim_mir::validate::validate_body;

/// The phase-specific post-condition check (M5.5 family 7): no
/// `Subslice` projection survives `slice_desugar`. Re-exported from
/// [`glyim_mir::validate`]; the pass manager runs it after `slice_desugar`
/// and after every subsequent pass.
pub use glyim_mir::validate::validate_no_subslice;

/// A single MIR well-formedness violation (site + kind + span). Re-exported
/// from the unified validator in [`glyim_mir::validate`].
pub use glyim_mir::validate::Violation;

/// The invariant a violation broke. Re-exported from
/// [`glyim_mir::validate`].
pub use glyim_mir::validate::ViolationKind;

/// Where a violation was found. Re-exported from [`glyim_mir::validate`].
pub use glyim_mir::validate::ViolationSite;
