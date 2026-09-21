use crate::AnalysisDatabase;
use crate::database::{FileMap, SourceMap};
use glyim_span::FileId;
use lsp_types::{RenameParams, TextEdit, WorkspaceEdit};
use std::str::FromStr;

/// Fallback used when the reference graph has no entries for `symbol_name`.
///
/// Lexes `source` and emits a `TextEdit` for every `SyntaxKind::Ident` token
/// whose text equals `symbol_name`. Crucially this skips occurrences inside
/// `StringLit`/`CharLit` tokens and comments (which the lexer excludes from
/// the token stream as trivia), so a name that also appears in a string
/// literal or comment is never corrupted.
///
/// Per Phase 8.2 (unstub-5) this is no longer the *primary* rename path — the
/// reference graph is authoritative for ordinary expressions. It is retained
/// as a production safety net for symbols the graph misses (e.g. variables
/// used only inside a macro call, which `lower_expr` currently drops — tracked
/// gap in KNOWN_GAPS.md), and is also exercised as a consistency check in
/// tests.
pub(crate) fn rename_text_fallback(
    sm: &SourceMap,
    file_id: FileId,
    symbol_name: &str,
    new_name: &str,
) -> Option<Vec<TextEdit>> {
    let source = sm.source();
    let lexed = glyim_frontend::lexer::lex(source, file_id);
    let mut edits = Vec::new();
    for tok in &lexed.tokens {
        if tok.kind == glyim_syntax::SyntaxKind::Ident
            && tok.text.as_str() == symbol_name
            && let Some(((start_line, start_col), (end_line, end_col))) =
                sm.span_to_position(tok.span.lo.to_usize(), tok.span.hi.to_usize())
        {
            edits.push(TextEdit {
                range: lsp_types::Range {
                    start: lsp_types::Position {
                        line: start_line as u32,
                        character: start_col as u32,
                    },
                    end: lsp_types::Position {
                        line: end_line as u32,
                        character: end_col as u32,
                    },
                },
                new_text: new_name.to_string(),
            });
        }
    }
    if edits.is_empty() { None } else { Some(edits) }
}

/// rename_symbol.
pub fn rename_symbol(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &RenameParams,
) -> Option<WorkspaceEdit> {
    use lsp_types::{Position, Range, Uri, WorkspaceEdit};
    use std::collections::HashMap;
    use url::Url;

    let uri = &params.text_document_position.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let source_maps = db.source_maps.read();

    let sm = source_maps.get(&file_id)?;
    let pos = params.text_document_position.position;
    let offset = sm.line_col_to_offset(pos.line as usize, pos.character as usize)?;
    let source = sm.source();

    // Find symbol name at cursor
    let chars: Vec<char> = source.chars().collect();

    let mut start = offset;
    let mut end = offset;
    while start > 0 && (chars[start - 1].is_alphabetic() || chars[start - 1] == '_') {
        start -= 1;
    }
    while end < chars.len() && (chars[end].is_alphabetic() || chars[end] == '_') {
        end += 1;
    }
    if start == end {
        return None;
    }
    let symbol_name = &source[start..end];

    // Primary path: use the reference graph. Per Phase 8.2 (unstub-5) this
    // graph is the authoritative rename source for ordinary expressions.
    // NOTE: macro-call arguments are NOT yet lowered into the HIR body (the
    // frontend drops `MacroCall` exprs in `lower_expr`), so a variable used
    // only inside a `println!(...)`-style invocation is absent from the graph.
    // The text fallback below is therefore retained as a safety net for those
    // cases (tracked gap — see KNOWN_GAPS.md). It is no longer the primary
    // path; we only reach it when the graph has no entries for the symbol.
    let ref_graph = db.reference_graph.read();

    let references = ref_graph.find_references(symbol_name);
    if !references.is_empty() {
        // We have semantic references, use them.
        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();

        for r in references {
            if let Some(ref_path) = file_map.path(r.file_id)
                && let Ok(ref_url) = Url::from_file_path(ref_path)
            {
                let ref_uri = Uri::from_str(ref_url.as_ref()).ok()?;
                if let Some(sm_ref) = source_maps.get(&r.file_id)
                    && let Some(((start_line, start_col), (end_line, end_col))) =
                        sm_ref.span_to_position(r.span.lo.to_usize(), r.span.hi.to_usize())
                {
                    let range = Range {
                        start: Position {
                            line: start_line as u32,
                            character: start_col as u32,
                        },
                        end: Position {
                            line: end_line as u32,
                            character: end_col as u32,
                        },
                    };
                    let edit = TextEdit {
                        range,
                        new_text: params.new_name.clone(),
                    };
                    changes.entry(ref_uri).or_default().push(edit);
                }
            }
        }
        if changes.is_empty() {
            return None;
        }
        return Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        });
    }

    // Fallback: text-based search within the current file only. Lex first and
    // only replace `Ident` tokens (string/char literals and comments are
    // skipped automatically by the lexer). Reached only when the reference
    // graph had no entries for `symbol_name` (e.g. a symbol used solely inside
    // a macro call, which the HIR lowering currently drops — tracked gap).
    let edits = rename_text_fallback(sm, file_id, symbol_name, &params.new_name)?;
    let mut changes = HashMap::new();

    changes.insert(uri.clone(), edits);
    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}
