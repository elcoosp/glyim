//! Metamorphic relations — plan M4.7 suite v1
//! (`docs/roadmaps/glyim-master-plan.md` §6 M4.7).
//!
//! A metamorphic relation states an invariant that must hold between a
//! program and a transformed (or differently-configured) run of it: the
//! observable behaviour — the process exit code — must be identical. The
//! suite re-uses the plan's table (§5.3 CO-6) at v1 scope: four relations
//! that run entirely in-process through the shared front half plus the
//! canonical interpreter seam:
//!
//! | Relation | Transformation / config | Oracle |
//! |---|---|---|
//! | [`OptLevelInvariance`] | `-O0` vs `-O2` | same exit |
//! | [`AlphaRenaming`] | consistent alpha-renaming of every identifier | same exit |
//! | [`ItemReordering`] | reverse the top-level item order | same exit |
//! | [`DeadCodeInjection`] | EMI-lite: statements under a provably-dead `if` | same exit |
//!
//! The relations consume the M4.3 generator's programs
//! (`glyim-smith`) as one input class and committed run-pass fixtures as
//! the other — "relations run nightly over fixtures + generated
//! programs" (plan M4.7 DoD); the CI-tier test keeps a small slice in PR
//! budget and the nightly job widens it.

use glyim_db::{CrateConfig, Database};
use glyim_frontend::lex;
use glyim_mir_interp::run_program;
use glyim_pipeline::{Pipeline, compile_file_to_mir};
use glyim_smith::SmithRng;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The in-process execution seam every relation shares: compile through
/// `compile_file_to_mir` (parse → typeck → lower → borrowck → optimize —
/// the same prepared compilation every other mode derives from) and run
/// the monomorphized bodies through `glyim_mir_interp::run_program`.
pub struct MetamorphicRunner {
    workdir: PathBuf,
    counter: AtomicUsize,
}

impl MetamorphicRunner {
    /// Creates a runner writing its scratch programs under `workdir`.
    #[must_use]
    pub fn new(workdir: PathBuf) -> Self {
        Self {
            workdir,
            counter: AtomicUsize::new(0),
        }
    }

    /// Runs `source` at `opt` and returns the process exit code, or the
    /// rendered diagnostics that made the program invalid.
    ///
    /// # Errors
    ///
    /// Rendered diagnostics when the program does not compile.
    pub fn exit_code(&self, source: &str, opt: u8) -> Result<i32, Vec<String>> {
        let index = self.counter.fetch_add(1, Ordering::SeqCst);
        let path: PathBuf = self
            .workdir
            .join(format!("metamorphic_case_{index}_{opt}.g"));
        fs::write(&path, source).expect("write scratch program");
        let mut db = Database::new(CrateConfig {
            name: format!("glyim_test_metamorphic_{index}"),
            target_triple: format!(
                "{}-unknown-{}",
                std::env::consts::ARCH,
                std::env::consts::OS
            ),
            opt_level: opt,
        });
        let compilation = match compile_file_to_mir(&mut db, &path, opt) {
            Ok(compilation) => compilation,
            Err(diagnostics) => {
                return Err(diagnostics.iter().map(ToString::to_string).collect());
            }
        };
        let entry_main = Pipeline::entry_main_local_id(&mut db, &path)
            .ok_or_else(|| vec!["no `fn main` entry point".to_string()])?;
        let bodies: Vec<_> = compilation.bodies.values().cloned().collect();
        let outcome = run_program(&bodies, compilation.ty_ctx.as_ref(), Some(entry_main));
        Ok(outcome.exit_code)
    }
}

/// Outcome of one relation over one program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelationOutcome {
    /// Baseline and variant agree.
    Invariant,
    /// The relation was violated: this is a compiler bug under the
    /// relation's transformation/config.
    Diverged {
        /// Baseline exit code.
        baseline_exit: i32,
        /// Variant exit code.
        variant_exit: i32,
        /// The variant source that diverged (for reproduction).
        variant_source: String,
    },
    /// The relation does not apply to this program (recorded, never
    /// silent — plan §3.3's witness-debt rule in miniature).
    Skipped {
        /// Why the relation does not apply.
        reason: String,
    },
    /// The variant did not even compile — also a violation (an
    /// equivalence-preserving transformation must preserve compilability).
    Invalid {
        /// The diagnostics that rejected the variant.
        diagnostics: Vec<String>,
        /// The variant source that failed to compile.
        variant_source: String,
    },
}

/// One metamorphic relation: a name plus the check that compares a
/// baseline run against the relation's variant run.
pub trait MetamorphicRelation {
    /// Stable relation name (report column).
    fn name(&self) -> &'static str;

    /// Runs the relation over `source`; `runner` supplies both runs.
    fn check(
        &self,
        source: &str,
        rng: &mut SmithRng,
        runner: &MetamorphicRunner,
    ) -> RelationOutcome;
}

/// `-O0` vs `-O2` on the same source (CO-6 row 1): the optimizer must
/// never change observable behaviour.
pub struct OptLevelInvariance;

impl MetamorphicRelation for OptLevelInvariance {
    fn name(&self) -> &'static str {
        "opt-level-invariance"
    }

    fn check(
        &self,
        source: &str,
        _rng: &mut SmithRng,
        runner: &MetamorphicRunner,
    ) -> RelationOutcome {
        let baseline = runner.exit_code(source, 0);
        let variant = runner.exit_code(source, 2);
        match (baseline, variant) {
            (Ok(baseline_exit), Ok(variant_exit)) if baseline_exit == variant_exit => {
                RelationOutcome::Invariant
            }
            (Ok(baseline_exit), Ok(variant_exit)) => RelationOutcome::Diverged {
                baseline_exit,
                variant_exit,
                variant_source: source.to_string(),
            },
            (Ok(_), Err(diagnostics)) => RelationOutcome::Invalid {
                diagnostics,
                variant_source: source.to_string(),
            },
            // A program that fails at `-O0` is a harness/fixture problem,
            // not an optimizer bug; the caller filters such inputs first.
            (Err(diagnostics), _) => RelationOutcome::Invalid {
                diagnostics,
                variant_source: source.to_string(),
            },
        }
    }
}

/// Consistent alpha-renaming (CO-6 row "hygiene/renaming"): rename every
/// identifier to a fresh `vrK` name through the real lexer, leaving
/// string/char literal contents untouched.
///
/// Skipped for sources that contain string or char literals: an inline
/// format-string argument can reference an identifier by name, and a
/// token-level rename cannot reach inside the literal — the relation
/// would be unsound there, so it declines instead (never silent).
pub struct AlphaRenaming;

impl MetamorphicRelation for AlphaRenaming {
    fn name(&self) -> &'static str {
        "alpha-renaming"
    }

    fn check(
        &self,
        source: &str,
        _rng: &mut SmithRng,
        runner: &MetamorphicRunner,
    ) -> RelationOutcome {
        let Some(variant) = rename_idents(source) else {
            return RelationOutcome::Skipped {
                reason: "source contains string or char literals".to_string(),
            };
        };
        run_transformed(source, variant, runner)
    }
}

/// Reverse the top-level item order (CO-6 row "item reordering"): item
/// order must not change resolution or behaviour.
pub struct ItemReordering;

impl MetamorphicRelation for ItemReordering {
    fn name(&self) -> &'static str {
        "item-reordering"
    }

    fn check(
        &self,
        source: &str,
        _rng: &mut SmithRng,
        runner: &MetamorphicRunner,
    ) -> RelationOutcome {
        let Some(variant) = reverse_items(source) else {
            return RelationOutcome::Skipped {
                reason: "source has fewer than two top-level items".to_string(),
            };
        };
        run_transformed(source, variant, runner)
    }
}

/// EMI-lite (CO-6 row "equivalence modulo inputs"): inject statements
/// under a provably-dead condition. The injected block must be lowered,
/// checked, and eliminated without changing behaviour.
pub struct DeadCodeInjection;

impl MetamorphicRelation for DeadCodeInjection {
    fn name(&self) -> &'static str {
        "dead-code-injection"
    }

    fn check(
        &self,
        source: &str,
        rng: &mut SmithRng,
        runner: &MetamorphicRunner,
    ) -> RelationOutcome {
        let marker = "fn main() -> i32 {";
        let Some(position) = source.find(marker) else {
            return RelationOutcome::Skipped {
                reason: "no `fn main` found".to_string(),
            };
        };
        let insert_at = position + marker.len();
        let payload = rng.range(1_000, 2_000_000);
        let mut variant = String::with_capacity(source.len() + 64);
        variant.push_str(&source[..insert_at]);
        let _ = writeln!(
            variant,
            "\n    if 0 == 1 {{ let _dead = {payload} + 654_321; }}"
        );
        variant.push_str(&source[insert_at..]);
        run_transformed(source, variant, runner)
    }
}

/// Every v1 relation, in plan-table order.
#[must_use]
pub fn all_relations() -> Vec<Box<dyn MetamorphicRelation>> {
    vec![
        Box::new(OptLevelInvariance),
        Box::new(AlphaRenaming),
        Box::new(ItemReordering),
        Box::new(DeadCodeInjection),
    ]
}

/// Runs the baseline (`-O0`) and the transformed variant, comparing exit
/// codes: an equivalence-preserving transformation must preserve both
/// compilability and behaviour.
fn run_transformed(source: &str, variant: String, runner: &MetamorphicRunner) -> RelationOutcome {
    match runner.exit_code(source, 0) {
        Ok(baseline_exit) => match runner.exit_code(&variant, 0) {
            Ok(variant_exit) if variant_exit == baseline_exit => RelationOutcome::Invariant,
            Ok(variant_exit) => RelationOutcome::Diverged {
                baseline_exit,
                variant_exit,
                variant_source: variant,
            },
            Err(diagnostics) => RelationOutcome::Invalid {
                diagnostics,
                variant_source: variant,
            },
        },
        Err(diagnostics) => RelationOutcome::Invalid {
            diagnostics,
            variant_source: source.to_string(),
        },
    }
}

/// Token-level alpha-renaming of **let-bound locals** — the sound v1
/// scope of the CO-6 "hygiene/renaming" relation.
///
/// A name is renamed iff (a) it is bound by a `let` somewhere in the
/// source, (b) it is not a primitive type name (which lex as `Ident`) or
/// `main` (the entry-point contract), (c) the occurrence is not a
/// call/method position (followed by `(`) and not a path/field position
/// (preceded by `.` or `::`) — std-lib method and type names cannot be
/// renamed from source, so those positions stay untouched. Async sources
/// are skipped: the state-machine desugar references lowering-internal
/// names this token-level view cannot see. Skips are recorded, never
/// silent (plan §3.3).
fn rename_idents(source: &str) -> Option<String> {
    let lexed = lex(source, FileId::from_usize(0));
    if !lexed.diagnostics.is_empty() {
        return None;
    }
    if lexed
        .tokens
        .iter()
        .any(|t| matches!(t.kind, SyntaxKind::StringLit | SyntaxKind::CharLit))
        || lexed.tokens.iter().any(|t| t.kind == SyntaxKind::KwAsync)
    {
        return None;
    }
    const FIXED_NAMES: [&str; 17] = [
        "bool", "char", "str", "String", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16",
        "u32", "u64", "u128", "main", "usize",
    ];
    // Pass 1: names bound by a `let` are the renameable set.
    let mut renameable: Vec<&str> = Vec::new();
    for (index, token) in lexed.tokens.iter().enumerate() {
        if token.kind == SyntaxKind::Ident
            && !FIXED_NAMES.contains(&token.text.as_str())
            && index > 0
            && lexed.tokens[index - 1].kind == SyntaxKind::KwLet
            && !renameable.contains(&token.text.as_str())
        {
            renameable.push(token.text.as_str());
        }
    }
    if renameable.is_empty() {
        return None;
    }
    // Pass 2: rename occurrences of the set, skipping call positions
    // (followed by `(`) and path/field positions (preceded by `.`/`::`).
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    for (index, token) in lexed.tokens.iter().enumerate() {
        let preceded_by_accessor = index > 0
            && matches!(
                lexed.tokens[index - 1].kind,
                SyntaxKind::Dot | SyntaxKind::ColonColon
            );
        let followed_by_call = lexed
            .tokens
            .get(index + 1)
            .is_some_and(|next| next.kind == SyntaxKind::LParen);
        if token.kind == SyntaxKind::Ident
            && renameable.contains(&token.text.as_str())
            && !preceded_by_accessor
            && !followed_by_call
        {
            let rank = renameable
                .iter()
                .position(|name| *name == token.text.as_str())
                .expect("name came from this set");
            replacements.push((
                token.span.lo.to_raw() as usize,
                token.span.hi.to_raw() as usize,
                format!("vr{rank}"),
            ));
        }
    }
    if replacements.is_empty() {
        return None;
    }
    let mut variant = source.to_string();
    // Replace right-to-left so earlier spans stay valid.
    replacements.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.0));
    for (lo, hi, name) in replacements {
        variant.replace_range(lo..hi, &name);
    }
    Some(variant)
}

/// Splits top-level items (brace-depth scan that skips literals and
/// comments) and reverses their order; `None` when there are fewer than
/// two items.
fn reverse_items(source: &str) -> Option<String> {
    let mut items: Vec<&str> = Vec::new();
    let mut depth = 0usize;
    let mut item_start: Option<usize> = None;
    let mut first_item_start: Option<usize> = None;
    let mut last_item_end = 0usize;
    let bytes = source.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let rest = &source[index..];
        if rest.starts_with("//") {
            let end = source[index..]
                .find('\n')
                .map_or(source.len(), |p| index + p);
            index = end;
            continue;
        }
        if rest.starts_with("/*") {
            let end = source[index..]
                .find("*/")
                .map_or(source.len(), |p| index + p + 2);
            index = end;
            continue;
        }
        if rest.starts_with('"') {
            // Skip a string literal (no escape-crossing scanner needed for
            // the item split: a `"` inside a string ends it or is escaped).
            let mut cursor = index + 1;
            while cursor < bytes.len() {
                if bytes[cursor] == b'\\' {
                    cursor += 2;
                } else if bytes[cursor] == b'"' {
                    cursor += 1;
                    break;
                } else {
                    cursor += 1;
                }
            }
            index = cursor;
            continue;
        }
        match bytes[index] {
            b'{' => {
                depth += 1;
                index += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && item_start.is_some() {
                    // Absorb the trailing newline so chunks re-join
                    // losslessly.
                    let start = item_start.take().expect("checked above");
                    let end = if bytes.get(index + 1) == Some(&b'\n') {
                        index + 2
                    } else {
                        index + 1
                    };
                    items.push(&source[start..end]);
                    last_item_end = end;
                }
                index += 1;
            }
            _ => {
                // An item opens at its first non-whitespace byte at depth
                // zero — the `fn`/`enum`/`struct` keyword, not the body
                // brace — so signature lines stay inside their chunk.
                if depth == 0 && item_start.is_none() && !bytes[index].is_ascii_whitespace() {
                    item_start = Some(index);
                    if first_item_start.is_none() {
                        first_item_start = Some(index);
                    }
                }
                index += 1;
            }
        }
    }
    if items.len() < 2 {
        return None;
    }
    let prefix = &source[..first_item_start.unwrap_or(0)];
    let tail = &source[last_item_end.min(source.len())..];
    let mut reversed: Vec<&str> = items;
    reversed.reverse();
    Some(format!("{prefix}{}{tail}", reversed.concat()))
}
