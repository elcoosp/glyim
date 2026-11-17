//! Document-color bench (LSP-COLOR).
//!
//! Dependency-free (`harness = false`), mirroring the `will_save` and
//! `file_operations` benches: measures the color-request path end-to-end
//! over a synthetic styled corpus — the parse-tree literal walk, the
//! color-site policy, the wire range mapping, and the picker's
//! presentation echo — so CI logs can track the per-request cost as the
//! color seam grows consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench document_color
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_frontend::parse_to_syntax;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::document_color::{color_presentations, document_colors};
use lsp_types::{
    Color, ColorPresentationParams, DocumentColorParams, PartialResultParams,
    TextDocumentIdentifier, Uri, WorkDoneProgressParams,
};
use url::Url;

const UNITS: usize = 64;

fn main() {
    // Build the corpus: `UNITS` styled lines (one color literal each),
    // interleaved with unstyled lines (string literals that spell no
    // color) — a realistic "GSX attribute values over a component file"
    // listing. Stored exactly like the driver stores it: file map, source
    // map, parse tree (green node).
    use std::fmt::Write as _;
    let mut source = String::with_capacity(UNITS * 48);
    source.push_str("fn main() -> i32 {\n");
    for i in 0..UNITS {
        let _ = write!(
            source,
            "    let fill_{i} = \"#{i:02x}90ff\";\n    let label = \"not-a-color\";\n"
        );
    }
    source.push_str("    0\n}\n");

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/document_color.gly");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));
    let green = parse_to_syntax(&source, file_id).green_node;
    db.syntaxes.write().insert(file_id, green);

    let uri = Uri::from_str(Url::from_file_path(&path).unwrap().as_ref()).unwrap();
    let color_params = DocumentColorParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    // Warm the stored facts once outside the measured loop: the bench
    // measures the color-request path, not the analysis driver.
    let colors =
        document_colors(&db, &file_map, &color_params).expect("the corpus has color sites");
    assert_eq!(colors.len(), UNITS, "one swatch per styled line");

    // The measured section, path 1: the full swatch listing per request.
    let requests = 200;
    let start = Instant::now();
    for _ in 0..requests {
        black_box(document_colors(&db, &file_map, black_box(&color_params)));
    }
    let listing = start.elapsed();
    println!(
        "documentColor listing ({UNITS} sites, {requests} requests): {:>10.1?} total, {:>8.1?}/request",
        listing,
        listing / requests
    );

    // The measured section, path 2: the picker's presentation echo for
    // one site — the site scan runs again, then the range match and the
    // spelling.
    let first = &colors[0].range;
    let presentation_params = ColorPresentationParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        color: Color {
            red: 59.0 / 255.0,
            green: 167.0 / 255.0,
            blue: 214.0 / 255.0,
            alpha: 1.0,
        },
        range: *first,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    black_box(color_presentations(
        &db,
        &file_map,
        black_box(&presentation_params),
    ));
    let start = Instant::now();
    for _ in 0..requests {
        black_box(color_presentations(
            &db,
            &file_map,
            black_box(&presentation_params),
        ));
    }
    let echo = start.elapsed();
    println!(
        "colorPresentation echo (1 site, {requests} requests):   {:>10.1?} total, {:>8.1?}/request",
        echo,
        echo / requests
    );
}
