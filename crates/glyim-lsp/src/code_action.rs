use crate::AnalysisDatabase;
use crate::database::FileMap;
use glyim_diag::{GlyimDiagnostic, StructuredDiagnosticData};
use lsp_types::*;
use std::collections::HashSet;

use url::Url;

fn collect_unused_imports(source: &str, used_names: &HashSet<String>) -> Vec<(String, Range)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut imports = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("use ") {
            let import_line = trimmed.trim_start_matches("use ").trim_end_matches(';');
            let import_name = import_line
                .split("::")
                .last()
                .unwrap_or(import_line)
                .to_string();
            let start_col = line.len() - trimmed.len();
            let end_col = line.len();
            let range = Range {
                start: Position {
                    line: line_idx as u32,
                    character: start_col as u32,
                },
                end: Position {
                    line: line_idx as u32,
                    character: end_col as u32,
                },
            };
            imports.push((import_name, range));
        }
    }

    // An import is unused iff its name has zero Read/Write references anywhere
    // in the indexed HIR (resolved via the reference graph, not text search —
    // this avoids false positives on shadowed names and false negatives on
    // names that appear only inside strings/comments).
    imports
        .into_iter()
        .filter(|(name, _)| !used_names.contains(name))
        .collect()
}

/// Extract the variant names + shapes from the structured payload of a
/// `non-exhaustive match` diagnostic (plan §5.2). Returns `None` when the
/// diagnostic carries no structured data, so callers fall back to prose
/// parsing (`parse_missing_variants`).
fn parse_missing_variant_shapes(
    diag: &GlyimDiagnostic,
) -> Option<Vec<(String, glyim_diag::VariantShape)>> {
    diag.structured
        .as_ref()
        .map(|StructuredDiagnosticData::MissingMatchVariants(shapes)| shapes.clone())
}

/// Build the match-arm pattern text for a missing variant given its shape
/// (plan §5.1). Unit variants need no bindings; tuple variants bind each field
/// with `_`; struct variants use a `{ .. }` rest pattern. All three forms
/// compile regardless of which fields are actually present.
fn variant_pattern(name: &str, shape: &glyim_diag::VariantShape) -> String {
    match shape {
        glyim_diag::VariantShape::Unit => name.to_string(),
        glyim_diag::VariantShape::Tuple(n) => {
            let wildcards = vec!["_"; *n].join(", ");
            format!("{name}({wildcards})")
        }
        glyim_diag::VariantShape::Struct(_) => format!("{name} {{ .. }}"),
    }
}

/// Extract the variant names listed in a `non-exhaustive match: missing
/// variants \`A\`, \`B\`` diagnostic message. Used as a fallback when the
/// diagnostic carries no structured payload (plan §5.2).
fn parse_missing_variants(message: &str) -> Option<Vec<String>> {
    let prefix = "missing variants ";
    let idx = message.find(prefix)?;
    let rest = &message[idx + prefix.len()..];
    let names: Vec<String> = rest
        .split(',')
        .filter_map(|part| {
            let trimmed = part.trim();
            // Each variant name is wrapped in backticks: `Name`.
            let s = trimmed.trim_start_matches('`').trim_end_matches('`').trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
        .collect();
    if names.is_empty() { None } else { Some(names) }
}

/// Scan `source` from `start` and return the byte offset of the matching
/// closing `}` of the `match` expression whose opening brace is at/after
/// `start`. Brace depth starts at 0; the first `}` that brings depth back to 0
/// closes the match. Returns `None` if no closing brace is found.
fn find_match_closing_brace(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth: i64 = 0;
    let mut started = false;
    let mut i = start.min(bytes.len());
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                started = true;
            }
            b'}' => {
                depth -= 1;
                if started && depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Parse `trait \`X\` is not implemented for \`Y\`` → `(X, Y)`.
fn parse_trait_not_implemented(message: &str) -> Option<(String, String)> {
    let tstart = message.find("trait `")?;
    let rest = &message[tstart + "trait `".len()..];
    let tend = rest.find('`')?;
    let trait_name = rest[..tend].to_string();
    let rest2 = &rest[tend + 1..];
    let fstart = rest2.find("for `")?;
    let rest3 = &rest2[fstart + "for `".len()..];
    let fend = rest3.find('`')?;
    let type_name = rest3[..fend].to_string();
    if trait_name.is_empty() || type_name.is_empty() {
        None
    } else {
        Some((trait_name, type_name))
    }
}

/// provide_code_actions.
pub fn provide_code_actions(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &CodeActionParams,
) -> Option<Vec<CodeActionOrCommand>> {
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let source_maps = db.source_maps.read();
    let source_map = source_maps.get(&file_id)?;
    let source = source_map.source();

    let used_names = {
        let ref_graph = db.reference_graph.read();
        ref_graph.used_symbols()
    };

    let mut actions = Vec::new();

    // --- Existing: remove unused import -------------------------------------
    let unused_imports = collect_unused_imports(source, &used_names);
    for (import_name, range) in unused_imports {
        let edit = TextEdit {
            range,
            new_text: String::new(),
        };
        let action = CodeAction {
            title: format!("Remove unused import: {}", import_name),
            kind: Some(CodeActionKind::QUICKFIX),
            edit: Some(WorkspaceEdit {
                changes: Some({
                    #[allow(clippy::mutable_key_type)]
                    let mut map = std::collections::HashMap::new();
                    map.insert(uri.clone(), vec![edit]);
                    map
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        actions.push(CodeActionOrCommand::CodeAction(action));
    }

    // --- Plan §22.1: diagnostics-driven code actions ------------------------
    // `db.diagnostics` holds the LSP-converted `Diagnostic`s (which cannot carry
    // the typed `structured` payload); `raw_diagnostics` holds the original
    // `GlyimDiagnostic`s in the same 1:1 order, so we correlate by index.
    let diag_guard = db.diagnostics.read();
    let raw_guard = db.raw_diagnostics.read();
    if let Some(diags) = diag_guard.get(&file_id) {
        let raw = raw_guard.get(&file_id);
        for (i, diag) in diags.iter().enumerate() {
            // "Add missing match arm(s)": triggered by a non-exhaustive match
            // diagnostic. Synthesize one arm per missing variant immediately
            // before the match's closing brace.
            if diag
                .message
                .starts_with("non-exhaustive match: missing variants")
            {
                // Plan §5.2: prefer the typed structured payload (carries each
                // variant's shape) so we synthesize an arity-correct arm; fall
                // back to prose-parsed names (unit-style arms) for diagnostics
                // that predate structured data.
                let shapes: Vec<(String, glyim_diag::VariantShape)> = raw
                    .and_then(|r: &Vec<GlyimDiagnostic>| {
                        r.get(i).and_then(parse_missing_variant_shapes)
                    })
                    .unwrap_or_else(|| {
                        parse_missing_variants(&diag.message)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|v| (v, glyim_diag::VariantShape::Unit))
                            .collect()
                    });
                if !shapes.is_empty() {
                    let start_offset = source_map.line_col_to_offset(
                        diag.range.start.line as usize,
                        diag.range.start.character as usize,
                    );
                    if let Some(close) =
                        start_offset.and_then(|o| find_match_closing_brace(source, o))
                    {
                        // Insert one arm per missing variant. `unimplemented!()`
                        // mirrors rustc's skeleton; the user fills in the body.
                        // The pattern shape (Unit / Tuple(n) / Struct) is taken
                        // from the diagnostic so the generated arm compiles
                        // (plan §5.1).
                        let mut arms = String::new();
                        for (v, shape) in &shapes {
                            let pattern = variant_pattern(v, shape);
                            arms.push_str(&format!("    {} => unimplemented!(),\n", pattern));
                        }
                        let (ins_line, ins_col) =
                            crate::uri::offset_to_position(source, close).unwrap();
                        let insert_pos = Position {
                            line: ins_line as u32,
                            character: ins_col as u32,
                        };
                        let range = Range {
                            start: insert_pos,
                            end: insert_pos,
                        };
                        let edit = TextEdit {
                            range,
                            new_text: arms,
                        };
                        let title = shapes
                            .iter()
                            .map(|(v, _): &(String, glyim_diag::VariantShape)| v.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let action = CodeAction {
                            title: format!("Add missing match arm(s): {}", title),
                            kind: Some(CodeActionKind::QUICKFIX),
                            edit: Some(WorkspaceEdit {
                                changes: Some({
                                    #[allow(clippy::mutable_key_type)]
                                    let mut map = std::collections::HashMap::new();
                                    map.insert(uri.clone(), vec![edit]);
                                    map
                                }),
                                ..Default::default()
                            }),
                            ..Default::default()
                        };
                        actions.push(CodeActionOrCommand::CodeAction(action));
                    }
                }
            }

            // "Generate impl": triggered by a `trait X is not implemented for Y`
            // diagnostic. Append `impl X for Y { }` at the end of the file.
            if diag.message.contains("is not implemented for `") {
                if let Some((trait_name, type_name)) = parse_trait_not_implemented(&diag.message) {
                    let lines: Vec<&str> = source.lines().collect();
                    let last_line = lines.len().saturating_sub(1) as u32;
                    let last_col = lines.last().map(|l| l.len()).unwrap_or(0) as u32;
                    let eof = Position {
                        line: last_line,
                        character: last_col,
                    };
                    let new_text = format!("\nimpl {} for {} {{\n}}\n", trait_name, type_name);
                    let edit = TextEdit {
                        range: Range {
                            start: eof,
                            end: eof,
                        },
                        new_text,
                    };
                    let action = CodeAction {
                        title: format!("Generate impl: impl {} for {}", trait_name, type_name),
                        kind: Some(CodeActionKind::QUICKFIX),
                        edit: Some(WorkspaceEdit {
                            changes: Some({
                                #[allow(clippy::mutable_key_type)]
                                let mut map = std::collections::HashMap::new();
                                map.insert(uri.clone(), vec![edit]);
                                map
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    };
                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
        }
    }

    if actions.is_empty() {
        None
    } else {
        Some(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::collect_unused_imports;
    use std::collections::HashSet;

    fn names(set: &[&str]) -> HashSet<String> {
        set.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_unused_import_flagged_when_no_reference() {
        // `HashMap` is imported but never referenced anywhere in the HIR.
        let src = "use std::collections::HashMap;\nfn main() { let x = 42; }\n";
        let used = names(&[]); // empty reference graph
        let unused = collect_unused_imports(src, &used);
        assert_eq!(unused.len(), 1, "import with no references must be flagged");
        assert_eq!(unused[0].0, "HashMap");
    }

    #[test]
    fn test_used_import_not_flagged() {
        // `Foo` is imported AND has a real reference (it appears in code), so
        // it must NOT be flagged as unused — even though the old text heuristic
        // would also have found it, the graph is now the source of truth.
        let src = "use mymod::Foo;\nfn main() { let x = Foo; }\n";
        let used = names(&["Foo"]);
        let unused = collect_unused_imports(src, &used);
        assert!(
            unused.is_empty(),
            "import with a real reference must NOT be flagged, got {:?}",
            unused
        );
    }

    #[test]
    fn test_import_name_in_string_is_still_unused() {
        // `Secret` is imported and only appears inside a string literal and a
        // comment — no real reference. The graph-based check correctly keeps
        // it flagged (the old substring heuristic would also flag it; the
        // point is the graph is the authority, not raw text).
        let src = "use mymod::Secret;\nfn main() { let s = \"Secret inside a string\"; /* Secret in comment */ }\n";
        let used = names(&[]);
        let unused = collect_unused_imports(src, &used);
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].0, "Secret");
    }

    #[test]
    fn test_only_unused_among_many_flagged() {
        let src = "use a::Used;\nuse b::Unused;\nfn main() { let x = Used; }\n";
        let used = names(&["Used"]);
        let unused = collect_unused_imports(src, &used);
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].0, "Unused");
    }

    #[test]
    fn test_parse_missing_variants() {
        let msg = "non-exhaustive match: missing variants `B`, `C`";
        let v = super::parse_missing_variants(msg).unwrap();
        assert_eq!(v, vec!["B".to_string(), "C".to_string()]);
    }

    #[test]
    fn test_parse_trait_not_implemented() {
        let msg = "trait `Display` is not implemented for `Point`";
        let (t, ty) = super::parse_trait_not_implemented(msg).unwrap();
        assert_eq!(t, "Display");
        assert_eq!(ty, "Point");
    }

    #[test]
    fn test_find_match_closing_brace() {
        // match x { A => 1, B => 2 }
        let src = "match x { A => 1, B => 2 }";
        let close = super::find_match_closing_brace(src, 0).unwrap();
        assert_eq!(close, src.len() - 1);
        // Nested braces: inner block must not confuse the scan.
        let src2 = "match x { A => { foo() }, B => 2 }";
        let close2 = super::find_match_closing_brace(src2, 0).unwrap();
        assert_eq!(close2, src2.len() - 1);
    }

    // Plan §5.1: the synthesized match-arm pattern must match the variant's
    // declared shape so the inserted arms compile.
    #[test]
    fn test_variant_pattern_shapes() {
        use glyim_diag::VariantShape;
        assert_eq!(super::variant_pattern("A", &VariantShape::Unit), "A");
        assert_eq!(super::variant_pattern("B", &VariantShape::Tuple(0)), "B()");
        assert_eq!(
            super::variant_pattern("C", &VariantShape::Tuple(2)),
            "C(_, _)"
        );
        assert_eq!(
            super::variant_pattern("D", &VariantShape::Struct(vec!["x".into(), "y".into()])),
            "D { .. }"
        );
    }

    // Plan §5.2: the structured payload round-trips through the diagnostic.
    #[test]
    fn test_structured_missing_variants_roundtrip() {
        use glyim_diag::{GlyimDiagnostic, StructuredDiagnosticData, VariantShape};
        let diag = GlyimDiagnostic::non_exhaustive_match(
            glyim_span::Span::DUMMY,
            &["B".to_string(), "C".to_string()],
            &[
                ("B".to_string(), VariantShape::Tuple(2)),
                ("C".to_string(), VariantShape::Struct(vec!["x".into()])),
            ],
        );
        let shapes = super::parse_missing_variant_shapes(&diag).unwrap();
        assert_eq!(
            shapes,
            vec![
                ("B".to_string(), VariantShape::Tuple(2)),
                ("C".to_string(), VariantShape::Struct(vec!["x".to_string()])),
            ]
        );
        assert!(matches!(
            diag.structured,
            Some(StructuredDiagnosticData::MissingMatchVariants(_))
        ));
    }
}
