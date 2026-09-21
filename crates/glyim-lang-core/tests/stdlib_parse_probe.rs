//! Probe: parse every embedded core `.g` module through the real glyim
//! frontend and report which parse cleanly. Maps the production-readiness gap
//! in the standard library (audit report §TIER-2 #6).

use glyim_frontend::parse_to_syntax;
use glyim_lang_core::{core_source, core_source_all};
use glyim_span::FileId;

fn parse(name: &str, src: &str) -> bool {
    let r = parse_to_syntax(src, FileId::from_raw(1));
    if r.diagnostics.is_empty() {
        true
    } else {
        eprintln!(
            "  [PARSE-FAIL] {}: {} diag(s) — first: {:?}",
            name,
            r.diagnostics.len(),
            r.diagnostics.first().map(|d| d.message.clone())
        );
        false
    }
}

#[test]
fn probe_core_modules_parse() {
    let mut pass = 0usize;
    let mut fail = 0usize;
    println!("=== core ===");
    for name in [
        "option", "result", "iter", "slice", "str", "cell", "mem", "ptr", "ops", "cmp", "marker",
        "panic", "hint", "convert", "default", "future",
    ] {
        let src = core_source(name).expect("core module present");
        if parse(name, src) {
            pass += 1;
        } else {
            fail += 1;
        }
    }
    println!("=== core_all (concatenated) ===");
    if parse("core_all", &core_source_all()) {
        pass += 1;
    } else {
        fail += 1;
    }
    println!("CORE-PARSE-PROBE: {} pass, {} fail", pass, fail);
    assert!(pass > 0);
}
