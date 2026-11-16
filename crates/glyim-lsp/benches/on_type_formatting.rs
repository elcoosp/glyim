//! On-type-formatting request bench (LSP-ON-TYPE-FORMAT).
//!
//! Dependency-free (`harness = false`), mirroring the `document_link`
//! bench: measures the on-type request path end-to-end over a synthetic
//! misindented corpus — the policy (bracket-token walk through the
//! `OnTypeTokenSource` seam, stack pairing, indent computation) and the
//! full wire request (URI → open-file lookup → parse-tree walk → wire
//! edit) — so CI logs can track the per-keystroke cost as the seam grows
//! consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench on_type_formatting
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::on_type_formatting::{ParseTreeOnTypeSource, on_type_formatting, on_type_indent};
use lsp_types::{
    DocumentOnTypeFormattingParams, FormattingOptions, TextDocumentIdentifier,
    TextDocumentPositionParams,
};
use url::Url;

/// One unit: a nested block whose inner closer carries a 12-space
/// auto-indent the editor guessed (the opener line sits at 4). The
/// engine's realistic per-keystroke workload: find the matching opener
/// through two levels of nesting, fix the run. The `fn` closer is
/// already correct — the corpus also carries a no-fix keystroke the
/// engine must answer honestly with none.
const UNIT_SOURCE: &str = "fn unit() -> i32 {\n    if cond {\n        0\n            }\n}\n";
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` nested blocks, parsed once, the database
    // populated exactly like the analysis driver does (the on-type
    // engine reads recorded parse trees only, so no lowering or typeck
    // pass is on its path).
    let mut source = String::with_capacity(UNITS * UNIT_SOURCE.len());
    for _ in 0..UNITS {
        source.push_str(UNIT_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/on_type.gly");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));
    let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
    db.syntaxes.write().insert(file_id, parse_result.green_node);

    // The typed position: the last unit's misindented inner `}` (12
    // spaces then the brace), reported *after* the character, `ch` = `}`.
    let typed_char = source
        .rfind("            }")
        .expect("corpus carries misindented closers")
        + 12;
    let sm = SourceMap::new(file_id, source.clone());
    let ((line, col), _) = sm
        .span_to_position(typed_char, typed_char)
        .expect("fixture offset is char-boundary safe");

    // Policy bench: bracket walk + stack pairing + indent computation
    // over the stored parse tree through the seam, no database locks —
    // the unit the policy tests pin. The syntaxes guard lives in this
    // block only, so the wire bench below starts from a lock-free
    // database.
    {
        let syntaxes = db.syntaxes.read();
        let green = syntaxes
            .get(&file_id)
            .expect("bench fixture stores its tree");
        let source_link = ParseTreeOnTypeSource::new(green);
        let text = source.clone();
        let t0 = Instant::now();
        let mut fixes = 0usize;
        for _ in 0..1_000 {
            let fix = on_type_indent(&source_link, &text, typed_char, '}');
            if fix.is_some() {
                fixes += 1;
            }
            black_box(&fix);
        }
        let policy = t0.elapsed();
        assert_eq!(fixes, 1_000, "the misindented closer is always fixable");
        println!(
            "policy compute : {:>9.1?}/call (1,000 calls, {fixes} fixes due)",
            policy / 1_000
        );
    }

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it (URI → open-file lookup →
    // parse-tree walk → wire edit).
    let uri = Url::from_file_path(&path)
        .ok()
        .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
        .expect("bench fixture path converts to a URI");
    let params = DocumentOnTypeFormattingParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: lsp_types::Position {
                line: line as u32,
                character: col as u32 + 1,
            },
        },
        ch: "}".to_string(),
        options: FormattingOptions::default(),
    };
    let t1 = Instant::now();
    let mut wire_hits = 0usize;
    for _ in 0..1_000 {
        let response = on_type_formatting(&db, &file_map, &params).expect("request finds a fix");
        wire_hits += response.len();
        black_box(&response);
    }
    let wire = t1.elapsed();
    println!(
        "wire request   : {:>9.1?}/call (1,000 calls, {wire_hits} edits mapped)",
        wire / 1_000
    );

    black_box(wire_hits);
}
