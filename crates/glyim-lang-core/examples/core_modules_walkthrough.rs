//! Consumer walkthrough: enumerate and dump the Glyim *core* library
//! modules — the exact source-access surface the frontend's stdlib
//! bootstrap reads.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lang-core --example core_modules_walkthrough
//! ```
//!
//! The crate's `.g` sources are the language's core library: the module
//! index (`core_modules`) and the per-module lookup (`core_source`) are
//! what `glyim-cli` and the pipeline's stdlib bootstrap resolve `Option`,
//! `Result`, and friends against, and `core_source_all` is the combined
//! prelude spelling. The walkthrough prints the module inventory with
//! per-module line counts, dumps one module's first lines, and asserts
//! the round-trip (`core_source` of every indexed name succeeds; the
//! assembled prelude contains every module header).

fn main() {
    let modules = glyim_lang_core::core_modules();
    println!("core library: {} module(s)", modules.len());
    for name in modules {
        let source = glyim_lang_core::core_source(name).expect("indexed module must resolve");
        println!("  {name:<8} {} line(s)", source.lines().count());
    }

    // One module's head, verbatim from the embedded `.g` source.
    let ops = glyim_lang_core::core_source("ops").expect("ops module must resolve");
    let head: Vec<&str> = ops.lines().take(6).collect();
    println!("\n--- core/ops.g (first {} lines) ---", head.len());
    for line in &head {
        println!("{line}");
    }

    // The combined prelude carries every module, in index order.
    let assembled = glyim_lang_core::core_source_all();
    for name in modules {
        assert!(
            assembled.contains(&format!("// === module: {name} ===")),
            "the assembled prelude must include {name}"
        );
    }
    println!(
        "\nassembled prelude: {} bytes, all {} modules present",
        assembled.len(),
        modules.len()
    );
}
