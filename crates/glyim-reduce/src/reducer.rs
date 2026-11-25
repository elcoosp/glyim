//! The CST delta-debugger (plan M4.5: "CST delta-debugging, token
//! fallback").
//!
//! The algorithm is a granularity ladder over the real parse tree, in
//! the classic delta-debugging shape: at each tier the current source is
//! chunked into deletable regions, every chunk is tried once per sweep,
//! a deletion is committed only when the driver seam still reports the
//! pinned failure, and a tier sweeps until it can commit nothing before
//! the next tier descends:
//!
//! 1. `"item"` — top-level CST nodes (functions, structs, enums, …).
//!    Deleting a whole item is parse-clean by construction.
//! 2. `"statement"` — the topmost statement/expression nodes inside
//!    function bodies (`LetStmt`, `ExprStmt`, `ReturnExpr`, control-flow
//!    expressions). This is the tier that reaches a defect hiding inside
//!    one function.
//! 3. `"token"` — the fallback: individual non-trivia tokens from the
//!    real lexer. Most single-token deletions do not re-parse, which is
//!    why this tier only fires when the CST tiers bottomed out and why a
//!    committed token deletion is rare — it exists for defects that are
//!    only visible below statement granularity.
//!
//! Deletion is monotone within a tier and the whole ladder iterates until
//! a full round commits nothing (the fixed point), bounded by a check
//! budget so a pathological predicate cannot spin. The fixed point is
//! *not* claimed to be globally minimal — delta debugging's minimality is
//! per-tier, and the doc contract says so honestly.

use crate::checks::ReproCheck;
use glyim_frontend::lex;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

/// Default ceiling on driver-seam calls per reduction. The BLOATED-style
/// fixtures need a few hundred; 5000 leaves headroom for large inputs
/// while guaranteeing termination even on a pathologically permissive
/// check.
const DEFAULT_BUDGET: usize = 5000;

/// One recorded sweep tier with committed deletions.
#[derive(Clone, Debug)]
pub struct ReductionPass {
    /// Granularity tier: `"item"`, `"statement"`, or `"token"`.
    pub granularity: &'static str,
    /// Deletions committed in this pass (each kept the pinned failure).
    pub committed: usize,
    /// The source as it stood after this pass' first committed deletion —
    /// the trace a reviewer replays to confirm every accepted step still
    /// reproduced.
    pub first_committed_source: Option<String>,
}

/// The reduction outcome: the fixed-point source plus the honest
/// bookkeeping of what the reducer did to get there.
#[derive(Clone, Debug)]
pub struct ReducedRepro {
    original: String,
    source: String,
    passes: Vec<ReductionPass>,
    check_calls: usize,
}

impl ReducedRepro {
    /// The fixed-point source: hand this to the gap registry as the
    /// witness fixture body (plan M4.5's "minimal repro + witness
    /// fixture + gap stub per new bug").
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Line count of the reduced source (the plan's < 40-lines bar is
    /// measured against this).
    #[must_use]
    pub fn line_count(&self) -> usize {
        count_lines(&self.source)
    }

    /// Line count of the input the reducer was handed.
    #[must_use]
    pub fn original_line_count(&self) -> usize {
        count_lines(&self.original)
    }

    /// The recorded sweep passes, in execution order.
    #[must_use]
    pub fn passes(&self) -> &[ReductionPass] {
        &self.passes
    }

    /// How many driver-seam calls the reduction consumed.
    #[must_use]
    pub fn check_calls(&self) -> usize {
        self.check_calls
    }
}

/// Counts `'\n'`-separated lines the way the plan's 40-line bar means
/// them: trailing whitespace-only tail does not count as an extra line.
fn count_lines(source: &str) -> usize {
    source.trim_end().lines().count().max(1)
}

/// The delta-debugging driver. Generic over the check so any driver
/// (exit-code pin, diagnostic pin, a future M4.4 differential driver)
/// drives the same ladder.
pub struct Reducer<'a, C: ReproCheck + ?Sized> {
    check: &'a C,
    budget: usize,
}

impl<'a, C: ReproCheck + ?Sized> Reducer<'a, C> {
    /// Drives reductions through `check`.
    #[must_use]
    pub fn new(check: &'a C) -> Self {
        Self {
            check,
            budget: DEFAULT_BUDGET,
        }
    }

    /// Overrides the default check-call ceiling (tests and drivers with
    /// expensive seams).
    #[must_use]
    pub fn with_budget(mut self, budget: usize) -> Self {
        self.budget = budget;
        self
    }

    /// Reduces `source` to the per-tier fixed point. If `source` does not
    /// reproduce the pinned failure it is returned unchanged — a reducer
    /// cannot invent a repro.
    #[must_use]
    pub fn reduce(&self, source: &str) -> ReducedRepro {
        let mut state = ReducedRepro {
            original: source.to_string(),
            source: source.to_string(),
            passes: Vec::new(),
            check_calls: 0,
        };
        if !self.try_check(&mut state, source) {
            return state;
        }
        // The ladder rounds: item → statement → token, repeated until a
        // full round commits nothing (deleting statement chunks can
        // unlock item chunks and vice versa — the predicate is not
        // monotone across tiers).
        loop {
            let before = state.source.len();
            self.run_tier(&mut state, "item", &item_chunks);
            self.run_tier(&mut state, "statement", &statement_chunks);
            self.run_tier(&mut state, "token", &token_chunks);
            if state.source.len() == before || state.check_calls >= self.budget {
                break;
            }
        }
        state
    }

    /// One seam call, tracked so the budget and the stats stay honest.
    fn try_check(&self, state: &mut ReducedRepro, source: &str) -> bool {
        state.check_calls += 1;
        self.check.still_reproduces(source)
    }

    /// Sweeps one granularity tier to exhaustion, recording one
    /// [`ReductionPass`] per committed deletion. A commit changes the
    /// source, which invalidates every remaining offset in the sweep, so
    /// each commit restarts the sweep from a fresh chunking of the new
    /// source — the classic delta-debugging restart.
    fn run_tier(
        &self,
        state: &mut ReducedRepro,
        granularity: &'static str,
        chunks: &dyn Fn(&str) -> Vec<(usize, usize)>,
    ) {
        loop {
            if state.check_calls >= self.budget {
                return;
            }
            let ranges = chunks(&state.source);
            let mut committed = 0usize;
            let mut first: Option<String> = None;
            for (lo, hi) in ranges {
                if state.check_calls >= self.budget {
                    break;
                }
                let candidate = splice(&state.source, lo, hi);
                if candidate.is_empty() || candidate == state.source {
                    continue;
                }
                if self.try_check(state, &candidate) {
                    if first.is_none() {
                        first = Some(candidate.clone());
                    }
                    state.source = candidate;
                    committed += 1;
                    break;
                }
            }
            if committed == 0 {
                return;
            }
            state.passes.push(ReductionPass {
                granularity,
                committed,
                first_committed_source: first,
            });
        }
    }
}

/// Removes `source[lo..hi]` exactly. Line-structural widening (indent
/// and blank-line swallowing) happens in [`line_aligned`] when the
/// chunkers build the ranges; the token tier passes raw token spans.
/// A degenerate range (which cannot arise from the chunkers' own
/// parses) yields the source unchanged rather than panicking on
/// arithmetic.
fn splice(source: &str, lo: usize, hi: usize) -> String {
    let hi = hi.min(source.len());
    if lo >= hi {
        return source.to_string();
    }
    let mut out = String::with_capacity(source.len() - (hi - lo));
    out.push_str(&source[..lo]);
    out.push_str(&source[hi..]);
    out
}

/// Top-level item chunks: every child node of the `SourceFile` root, with
/// its exact byte range in the source.
fn item_chunks(source: &str) -> Vec<(usize, usize)> {
    let parsed = glyim_frontend::parse_to_syntax(source, FileId::from_usize(0));
    parsed
        .root
        .children()
        .map(|node| {
            let range = node.text_range();
            line_aligned(source, range.start().into(), range.end().into())
        })
        .collect()
}

/// Statement chunks: the topmost statement/expression nodes — body
/// statements, returns, control flow — with ancestor/descendant overlaps
/// collapsed to the ancestor (deleting a `while` already deletes its
/// body; re-deleting the body afterwards would use a stale range). A
/// `Block` itself is never a chunk: blocks are the containers the tier
/// walks *into*, and deleting a whole body would shadow every statement
/// inside it.
fn statement_chunks(source: &str) -> Vec<(usize, usize)> {
    let parsed = glyim_frontend::parse_to_syntax(source, FileId::from_usize(0));
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    collect_topmost(source, &parsed.root, &mut ranges);
    ranges
}

/// Depth-first collection of topmost statement-tier nodes, descending
/// through blocks and other non-statement containers.
fn collect_topmost(
    source: &str,
    node: &glyim_syntax::SyntaxNode,
    ranges: &mut Vec<(usize, usize)>,
) {
    for child in node.children() {
        let kind = child.kind();
        if kind == SyntaxKind::Block {
            collect_topmost(source, &child, ranges);
        } else if is_statement_tier(kind) {
            let range = child.text_range();
            ranges.push(line_aligned(
                source,
                range.start().into(),
                range.end().into(),
            ));
        } else {
            collect_topmost(source, &child, ranges);
        }
    }
}

/// Widens a chunk to whole lines: the deletion starts at the chunk's own
/// line start and swallows the rest of its line plus any fully-blank
/// lines that follow, so removing an item or statement does not pile up
/// orphan indents and blank lines. Text after the swallowed region keeps
/// its own indentation (the swallowed span never crosses a non-blank
/// line).
fn line_aligned(source: &str, lo: usize, hi: usize) -> (usize, usize) {
    let bytes = source.as_bytes();
    let mut lo = lo;
    while lo > 0 && matches!(bytes[lo - 1], b' ' | b'\t') {
        lo -= 1;
    }
    let mut hi = hi;
    let mut cursor = hi;
    while cursor < bytes.len() && matches!(bytes[cursor], b' ' | b'\t') {
        cursor += 1;
    }
    if cursor < bytes.len() && bytes[cursor] == b'\n' {
        hi = cursor + 1;
        while bytes.get(hi) == Some(&b'\n') {
            hi += 1;
        }
    }
    (lo, hi)
}

/// The statement/expression kinds the statement tier may delete whole.
fn is_statement_tier(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LetStmt
            | SyntaxKind::ExprStmt
            | SyntaxKind::ReturnExpr
            | SyntaxKind::AssignExpr
            | SyntaxKind::IfExpr
            | SyntaxKind::WhileExpr
            | SyntaxKind::LoopExpr
            | SyntaxKind::ForExpr
            | SyntaxKind::MatchExpr
    )
}

/// Token chunks — the fallback tier: every non-trivia token from the real
/// lexer, one byte-range per token. Deleting one token usually breaks the
/// parse, so this tier commits only when the seam still reproduces.
fn token_chunks(source: &str) -> Vec<(usize, usize)> {
    lex(source, FileId::from_usize(0))
        .tokens
        .iter()
        .filter(|token| !token.kind.is_trivia())
        .map(|token| {
            (
                token.span.lo.to_raw() as usize,
                token.span.hi.to_raw() as usize,
            )
        })
        .collect()
}
