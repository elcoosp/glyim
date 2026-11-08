use crate::LspState;
use glyim_db::Database;
use std::path::PathBuf;

#[tokio::test]
async fn test_rename_symbol_updates_all_references() {
    let db = Database::new(glyim_db::CrateConfig {
        name: "test".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    });
    let mut state = LspState::new(db);
    state.start_driver();

    let path = PathBuf::from("/test/main.g");
    let content = r"
fn old_name() -> i32 { 42 }
fn main() { let x = old_name(); }
";
    state.did_open(path.clone(), content.to_string(), 1);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    {
        let analysis = state.analysis();
        let ref_graph = analysis.reference_graph.read();
        let refs = ref_graph.find_references("old_name");
        assert!(refs.len() >= 2);
    }

    state.did_close(&path);
}

#[test]
fn test_rename_text_fallback_skips_string_and_comment() {
    use crate::database::SourceMap;
    use crate::rename::rename_text_fallback;
    use glyim_span::FileId;

    // `target` appears three times: once as a real identifier, once inside a
    // string literal, and once inside a `//` comment. The fallback must rename
    // only the real identifier and leave the string/comment occurrences alone.
    let content =
        "let target = 1;\nlet s = \"target in a string\";\n// target in a comment\ntarget = 2;\n";
    let sm = SourceMap::new(FileId::from_raw(0), content.to_string());

    let edits = rename_text_fallback(&sm, FileId::from_raw(0), "target", "renamed")
        .expect("expected fallback edits for the real identifier occurrences");

    // Exactly two identifier occurrences: `let target` and `target = 2`.
    assert_eq!(edits.len(), 2, "string/comment occurrences must be skipped");

    // Both edits must target the span of the word "target", never inside the
    // string literal (line 1) or the comment (line 2).
    for e in &edits {
        let line = e.range.start.line;
        assert!(
            line == 0 || line == 3,
            "edit landed on line {line} — must be a real identifier, not string/comment"
        );
        assert_eq!(e.new_text, "renamed");
    }
}

#[test]
fn test_rename_text_fallback_target_only_in_string_is_none() {
    use crate::database::SourceMap;
    use crate::rename::rename_text_fallback;
    use glyim_span::FileId;

    // `target` appears ONLY inside a string literal and a comment — no real
    // identifier occurrence — so the fallback must produce no edits (and must
    // NOT corrupt the string/comment text).
    let content = "let s = \"target in a string\";\n// target in a comment\n";
    let sm = SourceMap::new(FileId::from_raw(0), content.to_string());

    let edits = rename_text_fallback(&sm, FileId::from_raw(0), "target", "renamed");
    assert!(
        edits.is_none(),
        "name only in string/comment must yield no fallback edits"
    );
}
