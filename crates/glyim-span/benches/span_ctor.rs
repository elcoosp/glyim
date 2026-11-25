//! Span construction bench (CT-3 seam, plan M3.2).
//!
//! Dependency-free (`harness = false`), mirroring the other seam benches:
//! measures the per-span cost of the checked [`Span::new`] constructor and
//! the [`Span::range`] bridge against the infallible [`Span::at`] point
//! constructor, so the CT-3 invariant checks (the `lo + len <= u32::MAX`
//! refusal, the unconditional `lo <= hi` assert) are pinned in numbers —
//! they must stay negligible next to parsing. Run with:
//!
//! ```text
//! cargo bench -p glyim-span --bench span_ctor
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};

const ITERATIONS: u32 = 1 << 20;

fn main() {
    let file = FileId::from_usize(0);
    let ctx = SyntaxContext::ROOT;

    // 1. Checked `(lo, len)` constructor — the CT-3 representation with the
    //    overflow refusal (`lo + len` past `u32::MAX` returns an error).
    let start = Instant::now();
    for i in 0..ITERATIONS {
        let len = (i & 0xff) + 1;
        let span = Span::new(file, ByteIdx::from_usize(i as usize), len, ctx)
            .expect("1 MiB of iterations stay inside the span range");
        black_box(span);
    }
    let checked_new = start.elapsed();

    // 2. The `(lo, hi)` bridge — the legacy-shaped call sites: the
    //    unconditional backwards-range assert runs here.
    let start = Instant::now();
    for i in 0..ITERATIONS {
        let lo = ByteIdx::from_usize(i as usize);
        let span = Span::range(file, lo, ByteIdx::from_usize(i as usize + 1), ctx);
        black_box(span);
    }
    let range_bridge = start.elapsed();

    // 3. Infallible point constructor — the floor every span shape pays.
    let start = Instant::now();
    for i in 0..ITERATIONS {
        let span = Span::at(file, ByteIdx::from_usize(i as usize), ctx);
        black_box(span);
    }
    let point_at = start.elapsed();

    // 4. Refusal path: every iteration takes the `SpanOverflow` error arm
    //    (the guard is the cost being measured, never the happy path).
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let refused = Span::new(
            file,
            ByteIdx::from_usize(ByteIdx::MAX as usize),
            u32::MAX,
            ctx,
        );
        debug_assert!(refused.is_err());
        let _ = black_box(refused);
    }
    let overflow_refusal = start.elapsed();

    let per_op = |d: std::time::Duration| d.as_nanos() as f64 / f64::from(ITERATIONS);
    println!("span_ctor bench ({ITERATIONS} iterations each);");
    println!(
        "  Span::new (checked, ok)   : {:>10.2} ns/span",
        per_op(checked_new)
    );
    println!(
        "  Span::range (bridge)      : {:>10.2} ns/span",
        per_op(range_bridge)
    );
    println!(
        "  Span::at (point floor)    : {:>10.2} ns/span",
        per_op(point_at)
    );
    println!(
        "  Span::new (refusal arm)   : {:>10.2} ns/span",
        per_op(overflow_refusal)
    );
    println!();
    println!("Contract: the checked-constructor overhead over the point");
    println!("constructor stays in the low single-digit nanoseconds — the");
    println!("CT-3 invariant checks are negligible next to parsing.");
}
