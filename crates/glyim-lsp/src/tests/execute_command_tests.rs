//! Database-backed tests for the execute-command request (LSP-EXEC-CMD).
//!
//! Mirrors `code_lens_tests.rs`: the real parse → def-map → HIR pipeline
//! populates the reference graph exactly like the driver does, and the
//! full two-request contract is exercised end-to-end — the lens resolve
//! emits the `glyim.showReferences` command, the client "executes" it
//! through `execute_command` (the function the router registers for
//! `workspace/executeCommand`), and the answer's locations agree with the
//! title the resolve rendered.

use crate::AnalysisDatabase;
use crate::code_lens::{SHOW_REFERENCES_COMMAND, code_lenses, resolve_code_lens};
use crate::database::FileMap;
use crate::database::SourceMap;
use crate::execute_command::{ExecuteCommandError, execute_command};
use glyim_core::{CrateId, Interner};
use glyim_def_map::build_def_map;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_span::FileId;
use lsp_types::{
    CodeLensParams, ExecuteCommandParams, PartialResultParams, TextDocumentIdentifier,
    WorkDoneProgressParams,
};
use serde_json::json;
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// `fn main() { helper(0); }` + `fn helper(x: i32) -> i32 { x }`: one
/// called helper (one non-definition reference) and one uncalled caller —
/// the two shapes a lens command executes against.
const DOC: &str = "fn main() {\n\
                   helper(0);\n\
                   }\n\
                   fn helper(x: i32) -> i32 {\n\
                   x\n\
                   }\n";

/// Populate `db` + `file_map` for the document: the driver's parse →
/// def-map → HIR pipeline, with the reference graph built exactly like
/// the driver builds it.
fn populate(db: &AnalysisDatabase, file_map: &mut FileMap, path: &PathBuf) -> FileId {
    let file_id = db.file_map.write().get_or_create(path);
    let caller_file_id = file_map.get_or_create(path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));
    let parse_result = parse_to_syntax(DOC, file_id);
    let mut interner = Interner::new();
    let (_def_map, _def_diags) =
        build_def_map(&parse_result.root, CrateId::from_raw(0), interner.clone());
    let (hir, _hir_diags) = lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.reference_graph
        .write()
        .build_from_hir(file_id, &hir, &interner);
    file_id
}

fn uri_for(path: &PathBuf) -> lsp_types::Uri {
    lsp_types::Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

/// Executing the resolved lens's command answers the call-site locations
/// the title promised: `helper` (the second lens, in source order)
/// resolves to "1 reference" and the execution answers exactly one
/// location in the same file.
#[test]
fn executing_the_resolved_lens_answers_the_promised_references() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/execute_command.gly");
    populate(&db, &mut file_map, &path);

    // Lenses are in source order: `main` is first, `helper` second.
    let lens_params = CodeLensParams {
        text_document: TextDocumentIdentifier {
            uri: uri_for(&path),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let lenses = code_lenses(&db, &file_map, &lens_params).expect("the doc carries items");
    let resolved = resolve_code_lens(&db, &file_map, &lenses[1]).expect("the lens resolves");
    let command = resolved
        .command
        .expect("the resolved lens carries a command");
    assert_eq!(command.command, SHOW_REFERENCES_COMMAND);
    assert_eq!(
        command.title, "1 reference",
        "the live count is the rendered title"
    );

    let params = ExecuteCommandParams {
        command: command.command,
        arguments: command
            .arguments
            .expect("the resolve command carries its arguments"),
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let locations =
        execute_command(&db, &file_map, &params).expect("the advertised command executes");
    assert_eq!(locations.len(), 1, "helper carries exactly one call site");
    assert_eq!(locations[0].uri, uri_for(&path));
    // The location anchors the call site: the reference record's extent
    // maps back into `helper(0);` on line 1.
    assert_eq!(locations[0].range.start.line, 1);
}

/// An uncalled item's lens resolves to "0 references" and its execution
/// answers the honestly-empty list — the count and the answer can never
/// disagree, because they read the same records.
#[test]
fn executing_an_unreferenced_lens_answers_the_empty_list() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/execute_command_zero.gly");
    populate(&db, &mut file_map, &path);

    // Lenses are in source order: `main` (the first item) is uncalled.
    let lens_params = CodeLensParams {
        text_document: TextDocumentIdentifier {
            uri: uri_for(&path),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let lenses = code_lenses(&db, &file_map, &lens_params).expect("the doc carries items");
    let resolved = resolve_code_lens(&db, &file_map, &lenses[0]).expect("the lens resolves");
    let title = resolved
        .command
        .as_ref()
        .map(|command| command.title.clone());
    assert_eq!(title.as_deref(), Some("0 references"));

    let params = ExecuteCommandParams {
        command: SHOW_REFERENCES_COMMAND.to_string(),
        arguments: resolved
            .command
            .and_then(|command| command.arguments)
            .unwrap_or_default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let locations =
        execute_command(&db, &file_map, &params).expect("the advertised command executes");
    assert!(locations.is_empty(), "0 references answers an empty list");
}

/// A command id the server never advertised names nothing executable —
/// the same honest JSON-RPC error the code-lens resolve answers for a
/// fabricated echo.
#[test]
fn unknown_commands_are_rejected() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/execute_command_unknown.gly");
    populate(&db, &mut file_map, &path);

    let params = ExecuteCommandParams {
        command: "glyim.notACommand".to_string(),
        arguments: vec![json!("file:///test/execute_command_unknown.gly"), json!({})],
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    assert_eq!(
        execute_command(&db, &file_map, &params),
        Err(ExecuteCommandError::UnknownCommand(
            "glyim.notACommand".to_string()
        ))
    );
}

/// Malformed argument payloads (the shapes no resolve ever emits) and
/// echoes that re-join against no live item-level definition are
/// rejected — never guessed around.
#[test]
fn malformed_arguments_and_unjoinable_echoes_are_rejected() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/execute_command_malformed.gly");
    populate(&db, &mut file_map, &path);

    let malformed = ExecuteCommandParams {
        command: SHOW_REFERENCES_COMMAND.to_string(),
        arguments: vec![json!(42)],
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    assert_eq!(
        execute_command(&db, &file_map, &malformed),
        Err(ExecuteCommandError::MalformedArguments)
    );

    // A well-formed payload whose URI names no open document.
    let unjoinable = ExecuteCommandParams {
        command: SHOW_REFERENCES_COMMAND.to_string(),
        arguments: vec![
            json!("file:///test/never-opened.gly"),
            json!({ "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 7 } }),
        ],
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    assert_eq!(
        execute_command(&db, &file_map, &unjoinable),
        Err(ExecuteCommandError::UnjoinableReference)
    );

    // A well-formed payload whose range start matches no item-level
    // definition of the open document (a mid-file offset).
    let wrong_offset = ExecuteCommandParams {
        command: SHOW_REFERENCES_COMMAND.to_string(),
        arguments: vec![
            json!("file:///test/execute_command_malformed.gly"),
            json!({ "start": { "line": 1, "character": 4 }, "end": { "line": 1, "character": 8 } }),
        ],
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    assert_eq!(
        execute_command(&db, &file_map, &wrong_offset),
        Err(ExecuteCommandError::UnjoinableReference)
    );
}

/// The wire contract the engine implements: the lsp-types request type's
/// METHOD string is pinned so an lsp-types upgrade that reshapes the
/// endpoint surfaces here, not on a client's screen.
#[test]
fn wire_method_is_pinned() {
    use lsp_types::request::Request;
    assert_eq!(
        lsp_types::request::ExecuteCommand::METHOD,
        "workspace/executeCommand"
    );
}
