//! Consumer walkthrough: enumerate and dump the Glyim *alloc* library
//! modules — the owning-container sources (`Box`, `Vec`, `String`, `Rc`)
//! layered on top of the core library.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lang-alloc --example alloc_modules_walkthrough
//! ```
//!
//! The crate's `.g` sources are the allocation layer of the standard
//! library: `alloc_source` resolves one module by name, `alloc_modules`
//! is the module index, and `alloc_source_all` is the combined spelling
//! the assembler consumes. The walkthrough prints the module inventory
//! with per-module line counts, dumps one module's first lines, and
//! asserts the round-trip (`alloc_source` of every indexed name
//! succeeds; the assembled source contains every module header).

fn main() {
    let modules = glyim_lang_alloc::alloc_modules();
    println!("alloc library: {} module(s)", modules.len());
    for name in modules {
        let source = glyim_lang_alloc::alloc_source(name).expect("indexed module must resolve");
        println!("  {name:<8} {} line(s)", source.lines().count());
    }

    // One module's head, verbatim from the embedded `.g` source.
    let boxed = glyim_lang_alloc::alloc_source("boxed").expect("boxed module must resolve");
    let head: Vec<&str> = boxed.lines().take(6).collect();
    println!("\n--- alloc/boxed.g (first {} lines) ---", head.len());
    for line in &head {
        println!("{line}");
    }

    // The combined source carries every module, in index order.
    let assembled = glyim_lang_alloc::alloc_source_all();
    for name in modules {
        assert!(
            assembled.contains(&format!("// === module: {name} ===")),
            "the assembled source must include {name}"
        );
    }
    println!(
        "\nassembled source: {} bytes, all {} modules present",
        assembled.len(),
        modules.len()
    );
}
