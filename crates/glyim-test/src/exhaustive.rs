//! Per-operator exhaustive oracle — plan M4.8
//! (`docs/roadmaps/glyim-master-plan.md` §6 M4.8).
//!
//! *"Per-operator exhaustive tests: every `BinOp`/`UnOp`/`Cast` × all
//! pairs of 8-bit values (exhaustive), sampled wider widths, across
//! const-eval, interpreter, LLVM-O0/O2"* with the `DoD` bar
//! *"signedness/shift/overflow disagreements = 0 or witnessed"*.
//!
//! The module generates **sweep programs** — one per
//! operator × operand-class — whose operands are *loop-carried*, so no
//! pass can constant-fold the operation away, and folds all 65536 (256²)
//! operand pairs into a checksum the process exits with. It then checks
//! the same program across the plan's executors:
//!
//! | Leg | Exhaustiveness | Where |
//! |---|---|---|
//! | interpreter | every pair, at `-O0` **and** `-O2`, against a Rust ground-truth model | PR tier + nightly |
//! | const-eval | sampled 256 pairs per op in PR tier; `GLYIM_EXHAUSTIVE_CONST=1` widens to every pair | nightly job |
//! | native LLVM | the same sweep programs at `-O0`/`-O2` | nightly job (`real-llvm`) |
//!
//! ## Pinned semantics (the ground-truth model)
//!
//! Every rule below was pinned by compiling and running a probe at HEAD
//! before the model was written; a deviation of the interpreter or the
//! optimizer from this table is exactly the "signedness/shift/overflow
//! disagreement" the `DoD` names:
//!
//! - **8-bit runtime arithmetic wraps** (two's complement,
//!   opt-invariant): `i8 100 + 100 → -56`, `u8 200 + 100 → 44`,
//!   `-(i8 -128) → -128`, `i8 -128 / -1 → -128`, `i8 -128 % -1 → 0`.
//! - **Division/remainder by zero panics at runtime** (exit 101).
//! - **Shift amounts** are taken as the shift operand's value widened to
//!   `i32` and **masked with `& 31`** (the dynamic-path semantics,
//!   verified against every constant- and variable-amount probe), then
//!   saturate: a masked amount `>= 8` makes left shifts yield `0` and
//!   right shifts yield `0` for unsigned operands or the sign-fill for
//!   negative signed operands (`i8 1 << -1` → `0` because `-1 & 31 = 31`;
//!   `i8 (a) << -128` → shift-by-`0` because `-128 & 31 = 0`; `i8 -128 >>
//!   15` → `-1`; `u8 255 >> 9` → `0`). Shift-by-`0` rows therefore equal
//!   the unshifted sums.
//! - **`as` casts are bit reinterpretations**: sign- or zero-extend to
//!   the target width or truncate (all ten integer targets verified for
//!   both `i8` and `u8` sources).
//! - **Comparisons / bitwise / logical** are the mathematical ones.
//! - **Const-eval boundary (documented, diagnosed — never silent)**:
//!   explicitly-typed operand arithmetic that overflows is a `[C0001]`
//!   rejection in const position (`(127 as i8) + (1 as i8)`), as are
//!   division/remainder by zero (`5 / 0`), division of `i8::MIN` by
//!   `-1`, and **negative shift amounts** (`1 << -1`) — while the
//!   runtime wraps or panics on the same expressions. Shift amounts
//!   `>= 8` fold wide-then-truncate and agree with the runtime's
//!   saturation (`i8::MIN << 7` → 0 both ways); remainder of `i8::MIN`
//!   by `-1` folds to `0` and agrees with the runtime wrap. These
//!   boundary pairs are excluded from the const sums and pinned by
//!   probes instead: the const evaluator keeps *rejecting* them, and a
//!   probe that suddenly compiles is a disagreement.
//!
//! The seam is the shared in-process one (`compile_file_to_mir` +
//! `glyim_mir_interp::run_program`, re-used from the metamorphic suite's
//! [`crate::metamorphic::MetamorphicRunner`]) — no LLVM toolchain needed
//! for the first two legs; the native leg lives in the test binary under
//! the `real-llvm` feature, mirroring the differential gate.

use crate::metamorphic::MetamorphicRunner;
use std::fmt::Write as _;

/// How many pairs the PR-tier const leg samples per operator.
const CONST_SAMPLE: u32 = 256;

/// One binary-operator sweep case: `op` over `signed` (i8) or unsigned
/// (u8) operand pairs — or, for [`BinarySweep::bool_domain`], over the
/// four boolean pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinarySweep {
    /// Operator family id (`"add"`, `"div"`, `"shl"`, …); logical
    /// operators are `"and-bool"`/`"or-bool"`.
    pub op_id: &'static str,
    /// Source operator symbol.
    pub op: &'static str,
    /// `true` → operands are `(a as i8)`/`(b as i8)`; `false` → the raw
    /// `u8` loop variables.
    pub signed: bool,
    /// `true` → the operand domain is the four boolean pairs (logical
    /// operators) instead of all 256×256 values.
    pub bool_domain: bool,
}

/// One unary-operator sweep case (one operand, 256 values — 2 for the
/// boolean `not`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnarySweep {
    /// Case id (`"neg-i8"`, `"not-u8"`, …).
    pub id: &'static str,
    /// Source operator symbol (`"-"`, `"!"`).
    pub op: &'static str,
    /// Operand expression producing one value per iteration; e.g.
    /// `"(a as i8)"`, `"a"`, `"(a != 0)"`.
    pub operand: &'static str,
    /// Break bound on the loop variable: `0` → wrap at 256 values, `2` →
    /// the boolean pair domain.
    pub bound: u8,
    /// `true` when `x` is a boolean (the fold reads it through
    /// `if x { 1 } else { 0 }`; bool-to-int casts are not supported by
    /// the pipeline).
    pub bool_result: bool,
}

/// One `as`-cast sweep case: every 8-bit source value reinterpreted into
/// each integer target type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastSweep {
    /// Case id (`"i8-to-u64"`, …).
    pub id: &'static str,
    /// Source expression (`"a"` for `u8`, `"(a as i8)"` for signed).
    pub from: &'static str,
    /// Target type name.
    pub to: &'static str,
}

/// Every binary arithmetic/bitwise/shift/comparison operator over both
/// 8-bit operand classes, plus the two logical operators over the
/// boolean pair domain — the plan's "every `BinOp`".
pub const BINARY_SWEEPS: &[BinarySweep] = &[
    binary("add", "+", true),
    binary("add-u8", "+", false),
    binary("sub", "-", true),
    binary("sub-u8", "-", false),
    binary("mul", "*", true),
    binary("mul-u8", "*", false),
    binary("div", "/", true),
    binary("div-u8", "/", false),
    binary("rem", "%", true),
    binary("rem-u8", "%", false),
    binary("bitand", "&", true),
    binary("bitand-u8", "&", false),
    binary("bitor", "|", true),
    binary("bitor-u8", "|", false),
    binary("bitxor", "^", true),
    binary("bitxor-u8", "^", false),
    binary("shl", "<<", true),
    binary("shl-u8", "<<", false),
    binary("shr", ">>", true),
    binary("shr-u8", ">>", false),
    binary("eq", "==", true),
    binary("eq-u8", "==", false),
    binary("ne", "!=", true),
    binary("ne-u8", "!=", false),
    binary("lt", "<", true),
    binary("lt-u8", "<", false),
    binary("gt", ">", true),
    binary("gt-u8", ">", false),
    binary("le", "<=", true),
    binary("le-u8", "<=", false),
    binary("ge", ">=", true),
    binary("ge-u8", ">=", false),
    BinarySweep {
        op_id: "and-bool",
        op: "&&",
        signed: false,
        bool_domain: true,
    },
    BinarySweep {
        op_id: "or-bool",
        op: "||",
        signed: false,
        bool_domain: true,
    },
];

/// Every unary operator the language types (probed: unary `-` is
/// rejected on `u8`), plus the boolean `not` — the plan's "every
/// `UnOp`".
pub const UNARY_SWEEPS: &[UnarySweep] = &[
    UnarySweep {
        id: "neg-i8",
        op: "-",
        operand: "(a as i8)",
        bound: 0,
        bool_result: false,
    },
    UnarySweep {
        id: "not-i8",
        op: "!",
        operand: "(a as i8)",
        bound: 0,
        bool_result: false,
    },
    UnarySweep {
        id: "not-u8",
        op: "!",
        operand: "a",
        bound: 0,
        bool_result: false,
    },
    UnarySweep {
        id: "not-bool",
        op: "!",
        operand: "(a != 0)",
        bound: 2,
        bool_result: true,
    },
];

/// Every `as`-cast from both 8-bit sources to all ten integer targets
/// (each verified to compile at HEAD) — the plan's "every `Cast`".
pub const CAST_SWEEPS: &[CastSweep] = &[
    cast("i8-to-i8", "(a as i8)", "i8"),
    cast("i8-to-u8", "(a as i8)", "u8"),
    cast("i8-to-i16", "(a as i8)", "i16"),
    cast("i8-to-u16", "(a as i8)", "u16"),
    cast("i8-to-i32", "(a as i8)", "i32"),
    cast("i8-to-u32", "(a as i8)", "u32"),
    cast("i8-to-i64", "(a as i8)", "i64"),
    cast("i8-to-u64", "(a as i8)", "u64"),
    cast("i8-to-usize", "(a as i8)", "usize"),
    cast("i8-to-isize", "(a as i8)", "isize"),
    cast("u8-to-i8", "a", "i8"),
    cast("u8-to-u8", "a", "u8"),
    cast("u8-to-i16", "a", "i16"),
    cast("u8-to-u16", "a", "u16"),
    cast("u8-to-i32", "a", "i32"),
    cast("u8-to-u32", "a", "u32"),
    cast("u8-to-i64", "a", "i64"),
    cast("u8-to-u64", "a", "u64"),
    cast("u8-to-usize", "a", "usize"),
    cast("u8-to-isize", "a", "isize"),
];

/// A sweep case: something that can produce its program text and its
/// ground-truth exit code.
pub trait SweepCase {
    /// Stable case id (report column).
    fn id(&self) -> &'static str;
    /// The sweep program text (loop-carried operands, checksum exit).
    fn source(&self) -> String;
    /// The ground-truth exit code from the pinned-semantics model; a
    /// runtime panic (division/remainder by zero) pins exit 101.
    fn ground_truth_exit(&self) -> i32;
}

impl SweepCase for BinarySweep {
    fn id(&self) -> &'static str {
        self.op_id
    }

    fn source(&self) -> String {
        let bound_break = if self.bool_domain { "2" } else { "0" };
        let lhs = if self.signed { "(a as i8)" } else { "a" };
        let rhs = if self.signed { "(b as i8)" } else { "b" };
        let fold = if self.result_is_bool() {
            "acc = acc + if x { 1 } else { 0 };"
        } else {
            "acc = acc + (x as i32);"
        };
        let expr = if self.bool_domain {
            format!("(a != 0) {} (b != 0)", self.op)
        } else {
            format!("{lhs} {} {rhs}", self.op)
        };
        let mut source = String::from(
            "fn main() -> i32 {\n    let mut acc: i32 = 0;\n    let mut a: u8 = 0;\n    while true {\n        let mut b: u8 = 0;\n        while true {\n            let x = ",
        );
        let _ = write!(
            source,
            "{expr};\n            {fold}\n            b = b + 1;\n            if b == {bound_break} {{ break; }}\n        }}\n        a = a + 1;\n        if a == {bound_break} {{ break; }}\n    }}\n    return acc;\n}}\n"
        );
        source
    }

    fn ground_truth_exit(&self) -> i32 {
        let bound = if self.bool_domain { 2 } else { 256 };
        let mut acc: i64 = 0;
        for a in 0..bound {
            for b in 0..bound {
                match pair_value(self.op, self.signed, a as u8, b as u8) {
                    PairValue::Panic => return 101,
                    PairValue::Value(v) => acc += v,
                }
            }
        }
        exit_byte(acc)
    }
}

impl SweepCase for UnarySweep {
    fn id(&self) -> &'static str {
        self.id
    }

    fn source(&self) -> String {
        let bound_break = if self.bound == 0 { "0" } else { "2" };
        let fold = if self.bool_result {
            "acc = acc + if x { 1 } else { 0 };"
        } else {
            "acc = acc + (x as i32);"
        };
        format!(
            "fn main() -> i32 {{\n    let mut acc: i32 = 0;\n    let mut a: u8 = 0;\n    while true {{\n            let x = {} {};\n            {fold}\n            a = a + 1;\n            if a == {bound_break} {{ break; }}\n    }}\n    return acc;\n}}\n",
            self.op, self.operand
        )
    }

    fn ground_truth_exit(&self) -> i32 {
        let bound = if self.bound == 0 { 256 } else { 2 };
        let mut acc: i64 = 0;
        for a in 0..bound {
            let value = match self.id {
                "neg-i8" => i64::from((a as i8).wrapping_neg()),
                "not-i8" => i64::from(!(a as i8)),
                "not-u8" => i64::from(!a),
                "not-bool" => i64::from(a == 0),
                other => unreachable!("unary sweep id {other} has no pinned rule"),
            };
            acc += value;
        }
        exit_byte(acc)
    }
}

impl SweepCase for CastSweep {
    fn id(&self) -> &'static str {
        self.id
    }

    fn source(&self) -> String {
        format!(
            "fn main() -> i32 {{\n    let mut acc: i32 = 0;\n    let mut a: u8 = 0;\n    while true {{\n            let x = {} as {};\n            acc = acc + (x as i64) as i32;\n            a = a + 1;\n            if a == 0 {{ break; }}\n    }}\n    return acc;\n}}\n",
            self.from, self.to
        )
    }

    fn ground_truth_exit(&self) -> i32 {
        let mut acc: i64 = 0;
        for a in 0..256u16 {
            acc += cast_value(self.from, self.to, a as u8) as i32 as i64;
        }
        exit_byte(acc)
    }
}

/// Outcome of one interpreter-leg sweep check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SweepOutcome {
    /// `-O0` and `-O2` agree with each other and with the pinned model.
    Invariant,
    /// The two opt levels disagree on the same program — the class the
    /// plan's M4.8 exists to catch.
    OptDivergence {
        /// Exit at `-O0`.
        opt0: i32,
        /// Exit at `-O2`.
        opt2: i32,
        /// The pinned model's exit.
        expected: i32,
    },
    /// Both opt levels ran but the checksum left the pinned model.
    GroundTruthMismatch {
        /// Observed exits `(opt0, opt2)`.
        observed: (i32, i32),
        /// The pinned model's exit.
        expected: i32,
    },
    /// The sweep program did not compile — never expected for a pinned
    /// surface member.
    Invalid {
        /// The rendered diagnostics.
        diagnostics: Vec<String>,
        /// The program that failed to compile.
        source: String,
    },
}

/// Outcome of one const-eval-leg check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstOutcome {
    /// The const-folded sums equal the ground truth on every safe pair,
    /// and every boundary probe kept its `[C0001]` rejection.
    Agrees {
        /// Safe pairs folded into the sum program.
        safe_pairs: usize,
        /// Boundary pairs pinned by probes.
        boundary_pairs: usize,
    },
    /// The const-folded sum disagreed with the ground truth.
    SumMismatch {
        /// The pinned model's exit for the safe-pair sum.
        expected: i32,
        /// The observed exit.
        observed: i32,
        /// The sum program that disagreed.
        source: String,
    },
    /// A boundary probe compiled (or stopped naming `C0001`) —
    /// const-eval stopped rejecting an expression the pinned boundary
    /// says it must reject.
    BoundaryNotDiagnosed {
        /// The boundary pair whose rejection no longer reproduces.
        pair: (u8, u8),
        /// The program that unexpectedly compiled.
        source: String,
    },
    /// The sum program failed to compile on pairs the pinned model says
    /// are const-safe.
    Invalid {
        /// The rendered diagnostics.
        diagnostics: Vec<String>,
        /// The program that failed to compile.
        source: String,
    },
}

/// Runs one interpreter-leg check: the sweep program at `-O0` and `-O2`,
/// both compared to each other and to the pinned model.
#[must_use]
pub fn check_sweep<C: SweepCase>(runner: &MetamorphicRunner, sweep: &C) -> SweepOutcome {
    let source = sweep.source();
    let expected = sweep.ground_truth_exit();
    let opt0 = runner.exit_code(&source, 0);
    let opt2 = runner.exit_code(&source, 2);
    match (opt0, opt2) {
        (Err(diagnostics), _) | (_, Err(diagnostics)) => SweepOutcome::Invalid {
            diagnostics,
            source,
        },
        (Ok(o0), Ok(o2)) if o0 != o2 => SweepOutcome::OptDivergence {
            opt0: o0,
            opt2: o2,
            expected,
        },
        (Ok(o0), Ok(_)) if o0 == expected => SweepOutcome::Invariant,
        (Ok(o0), Ok(o2)) => SweepOutcome::GroundTruthMismatch {
            observed: (o0, o2),
            expected,
        },
    }
}

/// The PR-tier const-pair sample: 256 strided pairs per operator (the
/// four boolean pairs for the logical domain); with
/// `GLYIM_EXHAUSTIVE_CONST=1` (the nightly widening) the full pair
/// space.
#[must_use]
pub fn sampled_const_pairs(sweep: &BinarySweep) -> Vec<(u8, u8)> {
    if sweep.bool_domain {
        return vec![(0, 0), (0, 1), (1, 0), (1, 1)];
    }
    if std::env::var("GLYIM_EXHAUSTIVE_CONST").is_ok() {
        let mut pairs: Vec<(u8, u8)> = Vec::with_capacity(65536);
        for a in 0..256u16 {
            for b in 0..256u16 {
                pairs.push((a as u8, b as u8));
            }
        }
        return pairs;
    }
    let mut pairs = Vec::with_capacity(CONST_SAMPLE as usize);
    for i in 0..CONST_SAMPLE {
        let a = (i % 256) as u8;
        let b = ((i.wrapping_mul(7).wrapping_add(13)) & 0xff) as u8;
        pairs.push((a, b));
    }
    pairs
}

/// Runs one const-eval-leg check over `pairs`: the const-sum program
/// over the const-safe pairs plus one probe per boundary family.
#[must_use]
pub fn check_const_leg(
    runner: &MetamorphicRunner,
    sweep: &BinarySweep,
    pairs: &[(u8, u8)],
) -> ConstOutcome {
    let mut safe: Vec<(u8, u8)> = Vec::new();
    let mut boundaries: Vec<(u8, u8)> = Vec::new();
    for &(a, b) in pairs {
        if const_safe(sweep.op, sweep.signed, a, b) {
            safe.push((a, b));
        } else {
            boundaries.push((a, b));
        }
    }
    let mut expected: i64 = 0;
    for &(a, b) in &safe {
        if let PairValue::Value(v) = pair_value(sweep.op, sweep.signed, a, b) {
            expected += v;
        }
    }
    let expected_exit = exit_byte(expected);
    let source = const_sum_source(sweep, &safe);
    let sum_check = match runner.exit_code(&source, 0) {
        Err(diagnostics) => Some(ConstOutcome::Invalid {
            diagnostics,
            source,
        }),
        Ok(observed) if observed != expected_exit => Some(ConstOutcome::SumMismatch {
            expected: expected_exit,
            observed,
            source,
        }),
        Ok(_) => None,
    };
    if let Some(outcome) = sum_check {
        return outcome;
    }
    // One probe per probed boundary family: the first boundary pair the
    // pinned `[C0001]` contract covers must keep its rejection.
    if let Some(&(a, b)) = boundaries
        .iter()
        .find(|&&(a, b)| const_probe_expected(sweep.op, sweep.signed, a, b))
    {
        let probe = boundary_probe_source(sweep, a, b);
        match runner.exit_code(&probe, 0) {
            Ok(_) => {
                return ConstOutcome::BoundaryNotDiagnosed {
                    pair: (a, b),
                    source: probe,
                };
            }
            Err(diagnostics) => {
                if !diagnostics.iter().any(|d| d.contains("C0001")) {
                    return ConstOutcome::BoundaryNotDiagnosed {
                        pair: (a, b),
                        source: probe,
                    };
                }
            }
        }
    }
    ConstOutcome::Agrees {
        safe_pairs: safe.len(),
        boundary_pairs: boundaries.len(),
    }
}

/// The two-value model for one operand pair under one operator.
enum PairValue {
    /// The pinned wrap-comparison value.
    Value(i64),
    /// The pair panics at runtime (division/remainder by zero).
    Panic,
}

/// Evaluates `(a op b)` under the pinned runtime semantics.
fn pair_value(op: &str, signed: bool, a: u8, b: u8) -> PairValue {
    match (op, signed) {
        ("&&", _) => PairValue::Value(i64::from((a != 0) && (b != 0))),
        ("||", _) => PairValue::Value(i64::from((a != 0) || (b != 0))),
        (_, true) => {
            let (x, y) = (a as i8, b as i8);
            let v = match op {
                "+" => i64::from(x.wrapping_add(y)),
                "-" => i64::from(x.wrapping_sub(y)),
                "*" => i64::from(x.wrapping_mul(y)),
                "/" => {
                    if y == 0 {
                        return PairValue::Panic;
                    }
                    i64::from(x.wrapping_div(y))
                }
                "%" => {
                    if y == 0 {
                        return PairValue::Panic;
                    }
                    i64::from(x.wrapping_rem(y))
                }
                "&" => i64::from(x & y),
                "|" => i64::from(x | y),
                "^" => i64::from(x ^ y),
                "<<" => {
                    let m = shift_mask(i64::from(y));
                    if m >= 8 {
                        0
                    } else {
                        i64::from(((x as i16) << m) as i8)
                    }
                }
                ">>" => {
                    let m = shift_mask(i64::from(y));
                    if m >= 8 {
                        i64::from(if x < 0 { -1i8 } else { 0i8 })
                    } else {
                        i64::from(x >> m)
                    }
                }
                "==" => i64::from(x == y),
                "!=" => i64::from(x != y),
                "<" => i64::from(x < y),
                ">" => i64::from(x > y),
                "<=" => i64::from(x <= y),
                ">=" => i64::from(x >= y),
                other => unreachable!("binary sweep op {other} has no pinned rule"),
            };
            PairValue::Value(v)
        }
        (_, false) => {
            let v = match op {
                "+" => i64::from(a.wrapping_add(b)),
                "-" => i64::from(a.wrapping_sub(b)),
                "*" => i64::from(a.wrapping_mul(b)),
                "/" => {
                    if b == 0 {
                        return PairValue::Panic;
                    }
                    i64::from(a / b)
                }
                "%" => {
                    if b == 0 {
                        return PairValue::Panic;
                    }
                    i64::from(a % b)
                }
                "&" => i64::from(a & b),
                "|" => i64::from(a | b),
                "^" => i64::from(a ^ b),
                "<<" => {
                    let m = shift_mask(i64::from(b)) as u32;
                    if m >= 8 { 0 } else { i64::from(a << m) }
                }
                ">>" => {
                    let m = shift_mask(i64::from(b)) as u32;
                    if m >= 8 { 0 } else { i64::from(a >> m) }
                }
                "==" => i64::from(a == b),
                "!=" => i64::from(a != b),
                "<" => i64::from(a < b),
                ">" => i64::from(a > b),
                "<=" => i64::from(a <= b),
                ">=" => i64::from(a >= b),
                other => unreachable!("binary sweep op {other} has no pinned rule"),
            };
            PairValue::Value(v)
        }
    }
}

/// The pinned cast semantics: bit reinterpretation into the target
/// width, returned as the fold's `i64` view.
#[must_use]
fn cast_value(from: &str, to: &str, a: u8) -> i64 {
    let source: i64 = if from != "a" {
        i64::from(a as i8)
    } else {
        i64::from(a)
    };
    match to {
        "i8" => source as i8 as i64,
        "u8" => source as u8 as i64,
        "i16" => source as i16 as i64,
        "u16" => source as u16 as i64,
        "i32" => source as i32 as i64,
        "u32" => source as u32 as i64,
        "i64" | "isize" => source,
        "u64" | "usize" => source as u64 as i64,
        other => unreachable!("cast target {other} has no pinned rule"),
    }
}

/// The pinned dynamic shift-amount semantics: the operand value widened
/// to `i32` and masked with `& 31` (verified against every constant- and
/// variable-amount probe: `1 << -1` → shift-by-31 → `0`; shift-by-`-128`
/// → shift-by-`0`; shift-by-`16` → shift-by-16 → saturates to `0`).
fn shift_mask(amount: i64) -> i64 {
    amount & 31
}

/// The process-exit byte the interpreter reports for a folded checksum:
/// every sweep's accumulator fits `i32` by construction (largest
/// contribution: `i8` mul's 16256 × 65536 ≈ 1.1e9), so the byte is the
/// low eight bits.
fn exit_byte(acc: i64) -> i32 {
    ((acc as i32) as u32 & 0xff) as i32
}

/// Comparison and logical operators produce boolean `x` values; their
/// folds read them through `if x { 1 } else { 0 }`.
fn result_is_bool(op: &str) -> bool {
    matches!(op, "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||")
}

impl BinarySweep {
    /// See [`result_is_bool`].
    #[must_use]
    pub fn result_is_bool(&self) -> bool {
        result_is_bool(self.op)
    }
}

/// Helper for the const tables.
const fn binary(op_id: &'static str, op: &'static str, signed: bool) -> BinarySweep {
    BinarySweep {
        op_id,
        op,
        signed,
        bool_domain: false,
    }
}

/// Helper for the cast table.
const fn cast(id: &'static str, from: &'static str, to: &'static str) -> CastSweep {
    CastSweep { id, from, to }
}

/// Is `(a, b)` const-safe for this operator — i.e. does the pinned
/// const-eval semantics agree with the runtime value? The `false` cases
/// are exactly the documented boundary: overflowing explicitly-typed
/// arithmetic, division/remainder by zero, division of `i8::MIN` by
/// `-1`, and negative shift amounts (all `[C0001]` rejections in const
/// position).
fn const_safe(op: &str, signed: bool, a: u8, b: u8) -> bool {
    match (op, signed) {
        ("&&", _) | ("||", _) => true,
        ("+" | "-" | "*", true) => {
            let (x, y) = (a as i8, b as i8);
            match op {
                "+" => x.checked_add(y).is_some(),
                "-" => x.checked_sub(y).is_some(),
                _ => x.checked_mul(y).is_some(),
            }
        }
        ("+" | "-" | "*", false) => match op {
            "+" => a.checked_add(b).is_some(),
            "-" => a.checked_sub(b).is_some(),
            _ => a.checked_mul(b).is_some(),
        },
        ("/", true) => {
            let (x, y) = (a as i8, b as i8);
            y != 0 && !(x == i8::MIN && y == -1)
        }
        ("/", false) => b != 0,
        ("%", true) => {
            let y = b as i8;
            y != 0
        }
        ("%", false) => b != 0,
        // Const shifts fold in a wide `i32` and truncate (probed: `1 << 9`
        // folds to `0` like the runtime's masked saturation; `4 << 41` and
        // every negative amount are `[C0001]` rejections). Signed pairs
        // with amounts `0..32` therefore agree with the runtime. The
        // u8-typed shift form is rejected outright by const-eval
        // ("const << requires integer operand and integer shift amount",
        // `[C0001]`) — surfaced by this oracle; every unsigned shift pair
        // is boundary.
        ("<<" | ">>", true) => {
            let y = b as i8;
            y >= 0 && y < 32
        }
        ("<<" | ">>", false) => false,
        _ => true,
    }
}

/// Is this boundary pair one of the *probed* `[C0001]` rejection
/// families (overflow, division/remainder by zero, division of
/// `i8::MIN` by `-1`, negative shift amounts)? Only these pairs get a
/// boundary probe; other exclusions (wide-fold shift amounts on the
/// unsigned form) are recorded as excluded without an asserted
/// expectation.
fn const_probe_expected(op: &str, signed: bool, a: u8, b: u8) -> bool {
    match (op, signed) {
        ("+" | "-" | "*", _) => !const_safe(op, signed, a, b),
        ("/", true) => {
            let (x, y) = (a as i8, b as i8);
            y == 0 || (x == i8::MIN && y == -1)
        }
        ("/" | "%", false) => b == 0,
        ("%", true) => {
            let y = b as i8;
            y == 0
        }
        ("<<" | ">>", true) => {
            let y = b as i8;
            y < 0
        }
        ("<<" | ">>", false) => true,
        _ => false,
    }
}

/// The const-sum program over `safe` pairs: one const item per pair,
/// each folded by the const evaluator, summed by `main`.
fn const_sum_source(sweep: &BinarySweep, safe: &[(u8, u8)]) -> String {
    let mut source = String::new();
    for (index, &(a, b)) in safe.iter().enumerate() {
        let _ = writeln!(source, "const C{index}: i32 = {};", pair_expr(sweep, a, b));
    }
    let _ = write!(source, "fn main() -> i32 {{\n    let mut acc: i32 = 0;\n");
    for index in 0..safe.len() {
        let _ = writeln!(source, "    acc = acc + C{index};");
    }
    let _ = write!(source, "    return acc;\n}}\n");
    source
}

/// One pair's expression, in the sweep case's operand class.
fn pair_expr(sweep: &BinarySweep, a: u8, b: u8) -> String {
    let a_term = if sweep.signed {
        format!("({a} as i8)")
    } else {
        format!("({a} as u8)")
    };
    let b_term = if sweep.signed {
        format!("({b} as i8)")
    } else {
        format!("({b} as u8)")
    };
    if sweep.bool_domain {
        format!("if ({a} != 0) {} ({b} != 0) {{ 1 }} else {{ 0 }}", sweep.op)
    } else if result_is_bool(sweep.op) {
        format!("if {a_term} {} {b_term} {{ 1 }} else {{ 0 }}", sweep.op)
    } else {
        format!("({a_term} {} {b_term}) as i32", sweep.op)
    }
}

/// One boundary probe: a single const item over the boundary pair, which
/// the pinned boundary says must be *rejected* with `[C0001]`.
fn boundary_probe_source(sweep: &BinarySweep, a: u8, b: u8) -> String {
    format!(
        "const C0: i32 = {};\nfn main() -> i32 {{ return C0; }}\n",
        pair_expr(sweep, a, b)
    )
}
