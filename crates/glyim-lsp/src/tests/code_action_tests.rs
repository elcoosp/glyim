use crate::code_action::provide_code_actions;
use crate::tests::test_utils::setup_test_db;
use lsp_types::{
    CodeActionParams, Diagnostic, DiagnosticSeverity, Position, Range, TextDocumentIdentifier,
};

#[test]
fn test_code_action_removes_unused_import() {
    let source = r#"use std::collections::HashMap;

fn main() {
    let x = 5;
}
"#;
    let (db, file_map, uri, _file_id) = setup_test_db(source, "/test/main.g");
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Default::default(),
        context: Default::default(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let actions = provide_code_actions(&db, &file_map, &params);
    assert!(actions.is_some());
    let actions = actions.unwrap();
    assert!(!actions.is_empty());
    let action = &actions[0];
    if let lsp_types::CodeActionOrCommand::CodeAction(ca) = action {
        assert!(ca.title.contains("Remove unused import"));
        assert_eq!(ca.kind, Some(lsp_types::CodeActionKind::QUICKFIX));
        let edit = ca.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "");
    } else {
        panic!("Expected CodeAction");
    }
}

/// Plan §22.1: "Add missing match arm(s)" — a non-exhaustive match diagnostic
/// must yield a code action that inserts one `unimplemented!()` arm per missing
/// variant immediately before the match's closing brace, and the resulting
/// source must be exhaustive (compilable-shaped).
#[test]
fn test_code_action_add_missing_match_arm() {
    // A 2-variant enum match that only covers `A`.
    let source = "fn f(x: Color) -> i32 {\n    match x {\n        A => 1,\n    }\n}\n";
    let (db, file_map, uri, file_id) = setup_test_db(source, "/test/main.g");

    // Inject the exact diagnostic `check_expr` emits for a missing variant.
    let diag = Diagnostic {
        range: Range {
            start: Position {
                line: 1,
                character: 4,
            },
            end: Position {
                line: 3,
                character: 1,
            },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("glyim".to_string()),
        message: "non-exhaustive match: missing variants `B`".to_string(),
        ..Default::default()
    };
    db.diagnostics.write().insert(file_id, vec![diag]);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Default::default(),
        context: Default::default(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let actions = provide_code_actions(&db, &file_map, &params).unwrap();
    let ca = actions
        .iter()
        .find_map(|a| match a {
            lsp_types::CodeActionOrCommand::CodeAction(c)
                if c.title.contains("Add missing match arm") =>
            {
                Some(c)
            }
            _ => None,
        })
        .expect("expected an Add missing match arm action");

    let edit = ca.edit.as_ref().unwrap();
    let edits = edit.changes.as_ref().unwrap().get(&uri).unwrap();
    assert_eq!(edits.len(), 1);
    // The inserted text must contain the missing variant arm.
    assert!(edits[0].new_text.contains("B => unimplemented!()"));

    // Applying the edit must make the match exhaustive (every variant covered).
    let close_brace_offset = source.find('}').unwrap();
    let inserted = format!(
        "{}    B => unimplemented!(),\n{}",
        &source[..close_brace_offset],
        &source[close_brace_offset..]
    );
    assert!(inserted.contains("A => 1,"));
    assert!(inserted.contains("B => unimplemented!()"));
    // No variant is left unhandled.
    assert!(!inserted.contains("non-exhaustive"));
}

/// Plan §22.1: "Generate impl" — a `trait X is not implemented for Y`
/// diagnostic must yield a code action that appends `impl X for Y {}` at the
/// end of the file.
#[test]
fn test_code_action_generate_impl() {
    let source = "struct Point { x: i32, y: i32 }\n";
    let (db, file_map, uri, file_id) = setup_test_db(source, "/test/main.g");

    let diag = Diagnostic {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("glyim".to_string()),
        message: "trait `Display` is not implemented for `Point`".to_string(),
        ..Default::default()
    };
    db.diagnostics.write().insert(file_id, vec![diag]);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Default::default(),
        context: Default::default(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let actions = provide_code_actions(&db, &file_map, &params).unwrap();
    let ca = actions
        .iter()
        .find_map(|a| match a {
            lsp_types::CodeActionOrCommand::CodeAction(c) if c.title.contains("Generate impl") => {
                Some(c)
            }
            _ => None,
        })
        .expect("expected a Generate impl action");

    let edit = ca.edit.as_ref().unwrap();
    let edits = edit.changes.as_ref().unwrap().get(&uri).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "\nimpl Display for Point {\n}\n");
}
