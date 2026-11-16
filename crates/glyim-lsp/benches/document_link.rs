//! Document-link request bench (LSP-DOC-LINK).
//!
//! Dependency-free (`harness = false`), mirroring the `implementation`
//! and `semantic_tokens` benches: measures the document-link request
//! path end-to-end over a synthetic include-heavy corpus — the policy
//! (include-call shapes through the `LinkLexSource` seam over the stored
//! parse trees) and the full wire request (URI → open-file lookup →
//! parse-tree token walk → literal resolution → wire mapping) — so CI
//! logs can track the per-request cost as the link seam grows consumers.
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench document_link
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::document_link::{ParseTreeLinkSource, document_links, link_targets};
use lsp_types::{
    DocumentLinkParams, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
};
use url::Url;

/// One unit: two include-macro call sites the policy must sift per
/// request, plus a non-file macro and a non-literal call the policy must
/// reject — the realistic mix an editor session shows.
const UNIT_SOURCE: &str = "include_str!(\"inc/header.g\");\n\
                           include_bytes!(\"inc/data.bin\");\n\
                           fn unit() -> i32 {\n\
                           env!(\"PATH\");\n\
                           include!(concat!(\"a\", \"b\"));\n\
                           0\n\
                           }\n";
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` include-call groups, parsed once, the
    // database populated exactly like the analysis driver does.
    let mut source = String::with_capacity(UNITS * UNIT_SOURCE.len());
    for _ in 0..UNITS {
        source.push_str(UNIT_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/document_link.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));
    // The driver's parse: the link engine reads recorded parse trees
    // only, so no lowering or typeck pass is on its path.
    let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
    db.syntaxes.write().insert(file_id, parse_result.green_node);

    // The document's directory: the bench corpus's relative targets
    // resolve under it (`/bench/inc/...`), never against a real
    // filesystem entry — resolution is a pure path computation.
    let doc_dir = path.parent().expect("bench fixture path has a parent");

    // Policy bench: the include-call shapes over the stored parse tree
    // through the seam, no database locks — the unit the policy tests
    // pin. The syntaxes guard lives in this block only, so the wire bench
    // below starts from a lock-free database.
    {
        let syntaxes = db.syntaxes.read();
        let green = syntaxes
            .get(&file_id)
            .expect("bench fixture stores its tree");
        let source_link = ParseTreeLinkSource::new(green);
        let t0 = Instant::now();
        let mut hits = 0usize;
        for _ in 0..1_000 {
            let links = link_targets(&source_link, doc_dir);
            hits += links.len();
            black_box(&links);
        }
        let policy = t0.elapsed();
        println!(
            "policy compute : {:>9.1?}/call (1,000 calls, {hits} links total)",
            policy / 1_000
        );
    };

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it (URI → open-file lookup →
    // parse-tree walk → literal resolution → wire mapping).
    let params = DocumentLinkParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&path)
                .ok()
                .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                .expect("bench fixture path converts to a URI"),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let t1 = Instant::now();
    let mut wire_hits = 0usize;
    for _ in 0..1_000 {
        let response = document_links(&db, &file_map, &params).expect("request finds links");
        wire_hits += response.len();
        black_box(&response);
    }
    let wire = t1.elapsed();
    println!(
        "wire request   : {:>9.1?}/call (1,000 calls, {wire_hits} links mapped)",
        wire / 1_000
    );

    black_box(wire_hits);
}
