//! Walkthrough: what the `async fn` / `.await` desugar produces for the
//! loop-await shapes.
//!
//! Glyim has no runtime in this crate — an `async fn` is desugared at HIR
//! lowering into a future struct + `impl Future` whose `poll` body is a
//! `Start`/`S0`/`Done` coroutine state machine. This example feeds the three
//! supported loop-await shapes (and the still-unsupported shape) through the
//! public lowering seam and prints the generated machine for each:
//!
//! * the GAP-0006 `loop`-shape closure — `.await` inside an unbounded
//!   `loop { .. }` body whose exit is a depth-0 `break acc`. The desugar
//!   normalizes the `loop` into a `while true` (a fresh literal-`true`
//!   condition; the body is carried over untouched, no rename, no increment
//!   step), so the machine's `S0` state carries the accumulator across a
//!   `Poll::Pending` and the `break` translation becomes the machine's
//!   `Done { result: acc }` transition;
//! * the GAP-0001 closure shape — `.await` inside a `for i in 0..n` body.
//!   The desugar normalizes the `for` into a `while` (synthetic
//!   `let mut __for_i` iterator binding + explicit increment step), so the
//!   machine's `S0` state carries the iterator and the accumulator across a
//!   `Poll::Pending`;
//! * the pre-existing `while i < n` shape, for comparison;
//! * a `for` over a non-range iterable, which the desugar still rejects
//!   with the loud async-v2 diagnostic (`T0060`, the remaining GAP-0006
//!   half) rather than silently miscompiling.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-hir --example async_loop_desugar
//! ```

use glyim_core::interner::Interner;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_span::FileId;

const PREAMBLE: &str = "
enum Poll<T> { Ready(T), Pending }
trait Future {
    type Output;
    fn poll(&mut self) -> Poll<Self::Output>;
}
async fn dep(x: i32) -> i32 { x }
";

const LOOP_AWAIT: &str = "
async fn spin() -> i32 {
    let mut acc = 0;
    let mut i = 0;
    loop {
        acc = acc + dep(i).await;
        i = i + 1;
        if i >= 3 {
            break acc;
        }
    }
}
";

const FOR_LOOP_AWAIT: &str = "
async fn spin(n: i32) -> i32 {
    let mut acc = 0;
    for i in 0..n {
        acc = acc + dep(i).await;
    }
    acc
}
";

const WHILE_LOOP_AWAIT: &str = "
async fn countdown(n: i32) -> i32 {
    let mut acc = 0;
    let mut i = 0;
    while i < n {
        acc = acc + dep(i).await;
        i = i + 1;
    }
    acc
}
";

const NON_RANGE_FOR_AWAIT: &str = "
async fn drain(v: i32) -> i32 {
    let mut acc = 0;
    for i in unknown_iter {
        acc = acc + dep(i).await;
    }
    acc
}
";

fn main() {
    for (label, src) in [
        ("loop-body await (GAP-0006 loop-shape closure)", LOOP_AWAIT),
        ("for-loop await (GAP-0001 closure shape)", FOR_LOOP_AWAIT),
        ("while-loop await (pre-existing shape)", WHILE_LOOP_AWAIT),
        ("non-range for await (still rejected)", NON_RANGE_FOR_AWAIT),
    ] {
        println!("=== {label} ===");
        let source = format!("{PREAMBLE}{src}");
        let root = parse_to_syntax(&source, FileId::BOGUS).root;
        let mut interner = Interner::new();
        let (hir, diags) = lower_crate_for_pipeline(&root, &mut interner);

        let errors: Vec<&str> = diags
            .iter()
            .filter(|d| d.is_error())
            .map(|d| d.message.as_str())
            .collect();
        if !errors.is_empty() {
            println!("  rejected with {} error diagnostic(s):", errors.len());
            for e in errors {
                println!("    {}", e.lines().next().unwrap_or(""));
            }
            println!();
            continue;
        }

        // Print every item the desugar generated for the async fn: the
        // `*FutureState` enum (Start/S0/Done variants carrying the fn
        // params, the loop locals and — for the `for` shape — the synthetic
        // `__for_i*` iterator), the `*Future` wrapper struct, and the
        // `impl Future` with the resumable poll body.
        for item in hir.items.iter() {
            let name = hir.interner.resolve(item.name).to_string();
            match &item.kind {
                glyim_hir::ItemKind::Enum(e) if name.contains("FutureState") => {
                    let variants: Vec<String> = e
                        .variants
                        .iter()
                        .map(|v| {
                            let fields: Vec<String> = v
                                .fields
                                .iter()
                                .map(|f| hir.interner.resolve(f.name).to_string())
                                .collect();
                            format!("{}({})", hir.interner.resolve(v.name), fields.join(", "))
                        })
                        .collect();
                    println!("  enum {name} {{ {} }}", variants.join(", "));
                }
                glyim_hir::ItemKind::Struct(_) if name.contains("Future") => {
                    println!("  struct {name} {{ state: <machine> }}");
                }
                glyim_hir::ItemKind::Impl(_) if name.contains("Future") => {
                    println!(
                        "  impl Future for {name} {{ fn poll -> genuine suspend/resume machine }}"
                    );
                }
                glyim_hir::ItemKind::Fn(f) if !f.is_async && !name.contains("main") => {
                    println!(
                        "  fn {name}(...) -> {name}Future {{ wrapper returning the machine }}"
                    );
                }
                _ => {}
            }
        }
        println!();
    }
}
