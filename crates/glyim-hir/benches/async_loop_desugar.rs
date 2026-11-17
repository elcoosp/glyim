//! Async loop-await desugar bench (async-desugar seam).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-diag` /
//! `glyim-ledger` benches: measures `parse_to_syntax` +
//! [`lower_crate_for_pipeline`] (the public seam that runs the raw HIR
//! lowering **and** the `async fn` / `.await` desugar) over three
//! single-item sources, so CI logs can track the desugar cost as the
//! loop-await shapes grow:
//!
//! * `desugar_for_loop_await` — the GAP-0001 closure shape (`for i in
//!   0..n { acc = acc + dep(i).await; }`): the normalization pass rewrites
//!   the `for` into the while shape, then builds the `Start`/`S0`/`Done`
//!   coroutine machine.
//! * `desugar_loop_await` — the GAP-0006 `loop`-shape closure (`loop {
//!   acc = acc + dep(i).await; ... }`): the normalization pass rewrites the
//!   `loop` into `while true` in place (a fresh literal-`true` condition,
//!   the body carried over untouched), then builds the same machine.
//! * `desugar_while_loop_await` — the pre-existing supported shape
//!   (`while i < n { ... }`), the normalization-free baseline.
//! * `lower_plain_fn` — the same body without `async`/`.await`: the
//!   raw-lowering floor the desugar benches are measured against.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-hir --bench async_loop_desugar
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_core::interner::Interner;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_span::FileId;

/// `async fn spin(n: i32) -> i32` awaiting inside the given loop body,
/// plus the `Poll`/`Future` preamble the desugar's naming convention
/// expects (`dep` -> `depFuture`).
fn async_loop_source(loop_body: &str) -> String {
    format!(
        r"
enum Poll<T> {{ Ready(T), Pending }}
trait Future {{
    type Output;
    fn poll(&mut self) -> Poll<Self::Output>;
}}
async fn dep(x: i32) -> i32 {{ x }}
async fn spin(n: i32) -> i32 {{
    let mut acc = 0;
{loop_body}
    acc
}}
fn main() -> i32 {{ 0 }}
"
    )
}

/// How many times to lower each source. The desugar is a few dozen
/// allocations per await, so a large iteration count keeps the wall-clock
/// reading above timer noise.
const ITERS: u32 = 2_000;

fn main() {
    let sources = [
        (
            "desugar_for_loop_await",
            async_loop_source("    for i in 0..n {\n        acc = acc + dep(i).await;\n    }"),
        ),
        (
            "desugar_loop_await",
            async_loop_source(
                "    let mut i = 0;\n    loop {\n        acc = acc + dep(i).await;\n        i = i + 1;\n        if i >= n {\n            break acc;\n        }\n    }",
            ),
        ),
        (
            "desugar_while_loop_await",
            async_loop_source(
                "    let mut i = 0;\n    while i < n {\n        acc = acc + dep(i).await;\n        i = i + 1;\n    }",
            ),
        ),
        (
            "lower_plain_fn",
            // The same accumulator loop, no `async`/`.await`: the raw
            // lowering floor (no desugar runs).
            "
fn acc_loop(n: i32) -> i32 {
    let mut acc = 0;
    for i in 0..n {
        acc = acc + i;
    }
    acc
}
fn main() -> i32 { 0 }
"
            .to_string(),
        ),
    ];

    // Contract pins: every source must lower with zero diagnostics, and the
    // two async shapes must each produce their coroutine state machine.
    for (name, src) in &sources {
        let root = parse_to_syntax(src, FileId::BOGUS).root;
        let mut interner = Interner::new();
        let (hir, diags) = lower_crate_for_pipeline(&root, &mut interner);
        assert!(
            diags.is_empty(),
            "{name}: lowering must be diagnostic-free, got {diags:?}"
        );
        let items: Vec<String> = hir
            .items
            .iter()
            .map(|it| hir.interner.resolve(it.name).to_string())
            .collect();
        if name.starts_with("desugar_") {
            assert!(
                items.iter().any(|n| n.contains("FutureState")),
                "{name}: desugar must produce a *FutureState machine; items: {items:?}"
            );
        }
        black_box(&hir);
    }

    for (name, src) in &sources {
        let root = parse_to_syntax(src, FileId::BOGUS).root;
        let started = Instant::now();
        for _ in 0..ITERS {
            let mut interner = Interner::new();
            let (hir, diags) = lower_crate_for_pipeline(black_box(&root), &mut interner);
            black_box(&hir);
            black_box(diags.is_empty());
        }
        let elapsed = started.elapsed();
        let per_call = elapsed.as_nanos() / u128::from(ITERS);
        println!("{name}: {ITERS} iters, {elapsed:?} total, ~{per_call} ns/call");
    }
}
