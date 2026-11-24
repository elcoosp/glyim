//! Policy-level tests for the execute-command engine (LSP-EXEC-CMD).
//!
//! Mirrors `code_lens.rs`: the *policy* (which argument shapes parse,
//! and what the reference walk answers) is pinned through an in-memory
//! [`MockSource`] implementing [`crate::execute_command::ReferenceLookup`],
//! decoupled from any reference-graph lock, source map, or wire type. The
//! graph-backed production path and the full wire request (the lens
//! command executed end-to-end) are covered in `execute_command_tests.rs`.

use crate::execute_command::{
    ReferenceLookup, ShowReferencesArgs, compute_reference_targets, parse_show_references_args,
};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use lsp_types::Position;
use serde_json::json;

/// In-memory reference store: the policy-side test double. Reference
/// locations are keyed by item name exactly like the production graph
/// lookup; the policy applies its own ordering, dedupe, and skip rules on
/// top. The item-name join is not part of the policy surface (it lives in
/// the entry point and is covered end-to-end), so this double answers
/// `None` for it.
#[derive(Default)]
struct MockSource {
    refs: Vec<(String, FileId, Span)>,
}

impl MockSource {
    fn reference(&mut self, name: &str, file: u32, lo: usize, hi: usize) {
        self.refs.push((
            name.to_string(),
            FileId::from_usize(file as usize),
            span_of(file, lo, hi),
        ));
    }
}

fn span_of(file: u32, lo: usize, hi: usize) -> Span {
    Span::new(
        FileId::from_usize(file as usize),
        ByteIdx::from_usize(lo),
        ByteIdx::from_usize(hi),
        SyntaxContext::ROOT,
    )
}

impl ReferenceLookup for MockSource {
    fn item_name_at(&self, _file_id: FileId, _offset: usize) -> Option<String> {
        // The policy tests never consult the item join (it is the entry
        // point's half of the seam, covered end-to-end); nothing here
        // records an item-level definition to answer with.
        None
    }
    fn references_of(&self, name: &str) -> Vec<crate::symbol_index::DefinitionLocation> {
        self.refs
            .iter()
            .filter(|(ref_name, _, _)| ref_name == name)
            .map(
                |(_, file_id, span)| crate::symbol_index::DefinitionLocation {
                    file_id: *file_id,
                    span: *span,
                },
            )
            .collect()
    }
}

/// The lens resolve contract's argument shape — `[uri, range]` — parses
/// into the policy's args.
#[test]
fn the_resolve_argument_shape_parses() {
    let args = parse_show_references_args(&[
        json!("file:///doc.gly"),
        json!({
            "start": { "line": 0, "character": 3 },
            "end": { "line": 0, "character": 7 },
        }),
    ])
    .expect("the advertised shape parses");
    assert_eq!(args.uri, "file:///doc.gly");
    assert_eq!(
        args.range.start,
        Position {
            line: 0,
            character: 3
        }
    );
    assert_eq!(
        args.range.end,
        Position {
            line: 0,
            character: 7
        }
    );
}

/// Everything a client can get wrong — no arguments, one argument, a
/// non-string URI, a non-range second argument — parses to nothing; the
/// caller answers a JSON-RPC error instead of guessing.
#[test]
fn malformed_argument_shapes_parse_to_none() {
    assert!(parse_show_references_args(&[]).is_none(), "no arguments");
    assert!(
        parse_show_references_args(&[json!("file:///doc.gly")]).is_none(),
        "one argument"
    );
    assert!(
        parse_show_references_args(&[json!(42), json!({})]).is_none(),
        "non-string uri"
    );
    assert!(
        parse_show_references_args(&[json!("file:///doc.gly"), json!("not-a-range")]).is_none(),
        "non-range second argument"
    );
    assert!(
        parse_show_references_args(&[json!("file:///doc.gly"), json!({ "start": 1 })]).is_none(),
        "range missing its end"
    );
}

/// The echo round-trips: `ShowReferencesArgs` built from the wire shapes
/// the code-lens resolve emits deserialize against the same field names.
#[test]
fn args_round_trip_through_the_wire_shape() {
    let original = ShowReferencesArgs {
        uri: "file:///w.gly".to_string(),
        range: lsp_types::Range {
            start: Position {
                line: 2,
                character: 5,
            },
            end: Position {
                line: 2,
                character: 9,
            },
        },
    };
    let wire = json!({
        "uri": original.uri,
        "range": original.range,
    });
    let parsed = parse_show_references_args(&[json!(wire["uri"]), json!(wire["range"])])
        .expect("the round-trip shape parses");
    assert_eq!(parsed, original);
}

/// Reference targets: ordered by (file id, span start) regardless of the
/// source's walk order, with duplicates collapsed.
#[test]
fn reference_targets_are_ordered_and_deduped() {
    let mut mock = MockSource::default();
    mock.reference("helper", 1, 0, 6);
    mock.reference("helper", 0, 44, 50);
    mock.reference("helper", 0, 44, 50);
    let targets = compute_reference_targets(&mock, "helper");
    assert_eq!(targets.len(), 2, "duplicate record collapses");
    assert_eq!(targets[0].file_id, FileId::from_usize(0));
    assert_eq!(targets[0].span.lo.to_usize(), 44);
    assert_eq!(targets[1].file_id, FileId::from_usize(1));
}

/// A zero-width recorded extent cannot name a jump target and is
/// skipped; a name with no recorded references answers an empty walk.
#[test]
fn reference_targets_skip_zero_width_extents() {
    let mut mock = MockSource::default();
    mock.reference("helper", 0, 40, 40);
    mock.reference("helper", 0, 50, 56);
    let targets = compute_reference_targets(&mock, "helper");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].span.lo.to_usize(), 50);

    assert!(compute_reference_targets(&mock, "unknown").is_empty());
}

/// The parsed URI is the client's echo, verbatim — the engine resolves it
/// against the session's file map, never against the filesystem layout
/// the server happens to run in.
#[test]
fn parsed_uri_is_the_client_echo_verbatim() {
    let args = parse_show_references_args(&[
        json!("file:///w.gly"),
        json!({
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 1 },
        }),
    ])
    .expect("parses");
    assert_eq!(args.uri, "file:///w.gly");
}
