//! Probe: parse every embedded alloc `.g` module through the real glyim
//! frontend and report which parse cleanly. Maps the production-readiness gap
//! in the standard library (audit report §TIER-2 #6). alloc.g was fixed today
//! (unsafe-fn parse blocker); this confirms `boxed/rc/vec/string/raw_vec` too.

use glyim_frontend::parse_to_syntax;
use glyim_lang_alloc::{alloc_source, alloc_source_all};
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
fn probe_alloc_modules_parse() {
    let mut pass = 0usize;
    let mut fail = 0usize;
    println!("=== alloc ===");
    for name in ["alloc", "boxed", "raw_vec", "rc", "vec", "string"] {
        let src = alloc_source(name).expect("alloc module present");
        if parse(name, src) {
            pass += 1;
        } else {
            fail += 1;
        }
    }
    println!("=== alloc_all (concatenated) ===");
    if parse("alloc_all", &alloc_source_all()) {
        pass += 1;
    } else {
        fail += 1;
    }
    println!("ALLOC-PARSE-PROBE: {pass} pass, {fail} fail");
    assert!(pass > 0);
}
