//! Execute-command request support (feature tag LSP-EXEC-CMD).
//!
//! The code-lens engine (LSP-CODE-LENS) resolves every lens to the
//! `glyim.showReferences` command — a namespaced command id a client is
//! meant to execute — but the server never answered
//! `workspace/executeCommand`: the command the advertised capability
//! pointed at had no execution route, the exact "advertised but
//! unreachable" shape the conformance audit exists for. This module
//! closes that loop: the command the resolve emits is now executable
//! through the one route the LSP reserves for it.
//!
//! The seam: *which item a command targets and how often it is used* is
//! split from *how those facts are found* by [`ReferenceLookup`],
//! mirroring LSP-CODE-LENS's `LensSource`. The policy
//! ([`parse_show_references_args`], [`compute_reference_targets`]) reads
//! through the trait and stays testable without a reference-graph lock
//! (`tests/execute_command.rs`); the production [`GraphReferenceLookup`]
//! reads the driver's shared [`ReferenceGraph`] — the same store the
//! lens resolve reads — so a executed command's locations agree with the
//! title the resolve rendered by construction: both walk the same
//! non-definition records of the same item-level name.
//!
//! Wire mapping (LSP spec): `workspace/executeCommand` carries the
//! command id plus the resolve's argument payload verbatim —
//! `[uri-as-string, lens range]` ([`ShowReferencesArgs`]). The range
//! start maps back to a byte offset through the same `SourceMap` the
//! lens request mapped it from, and it must match a live item-level
//! definition of the open document (the LSP-CALL-HIERARCHY echo join) —
//! the command executes the lens the server handed out, never a
//! fabricated position. The answer is the referenced locations — the
//! same name-keyed non-definition records the resolve counted — as the
//! request's JSON value, ordered by (file id, span start) so the answer
//! is deterministic regardless of the graph's per-name map iteration
//! order. An uncalled item's command honestly answers an empty list (the
//! resolve title said "0 references", so an empty answer *is* the
//! count), and every failure mode is a named error — unknown command id,
//! malformed argument payload, an echo that re-joins against no live
//! item — surfaced as a JSON-RPC error by the router, never a guess.

use crate::AnalysisDatabase;
use crate::code_lens::SHOW_REFERENCES_COMMAND;
use crate::code_lens::{GraphLensSource, LensSource};
use crate::database::FileMap;
use crate::reference_graph::ReferenceGraph;
use crate::symbol_index::DefinitionLocation;
use glyim_span::FileId;
use lsp_types::{ExecuteCommandParams, Location, Range};
use serde_json::Value;
use std::fmt;
use std::str::FromStr;
use url::Url;

/// The arguments the code-lens resolve attaches to its command, carried
/// verbatim through the client's `workspace/executeCommand` echo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowReferencesArgs {
    /// The document URI the resolved lens named (the resolve payload's
    /// first argument, the only carrier of the document on this path).
    pub uri: String,
    /// The lens range the resolve echoed back (the payload's second
    /// argument; its start anchors the item-level definition).
    pub range: Range,
}

/// Why an `executeCommand` request could not be answered (LSP-EXEC-CMD).
///
/// The display text names the failed step — the router maps it verbatim
/// into the JSON-RPC error message, so a client chasing a rejected
/// command sees *what* failed, not just that something did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecuteCommandError {
    /// The command id is not one the server advertised. The offending id
    /// is preserved.
    UnknownCommand(String),
    /// The argument payload is not the shape the resolve emits (missing
    /// arguments, a non-string URI, a non-range second argument).
    MalformedArguments,
    /// The payload is well-formed but re-joins against nothing: the URI
    /// names no open document, its range start maps to no byte offset,
    /// or no live item-level definition starts there.
    UnjoinableReference,
}

impl fmt::Display for ExecuteCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand(command) => write!(
                f,
                "workspace/executeCommand: unknown command `{command}` (advertised: \
                 {SHOW_REFERENCES_COMMAND})"
            ),
            Self::MalformedArguments => write!(
                f,
                "workspace/executeCommand: arguments are not the [uri, range] payload the \
                 {SHOW_REFERENCES_COMMAND} resolve emits"
            ),
            Self::UnjoinableReference => write!(
                f,
                "workspace/executeCommand: the echoed range matches no live item-level \
                 definition of an open document"
            ),
        }
    }
}

/// The command-target and reference-lookup seam for execute-command
/// requests (LSP-EXEC-CMD).
///
/// The policies in [`parse_show_references_args`] and
/// [`compute_reference_targets`] must stay testable without a live
/// `AnalysisDatabase` (reference-graph locks, source maps), so they read
/// the recorded facts through this trait. The production
/// [`GraphReferenceLookup`] reads the driver's shared [`ReferenceGraph`];
/// a future reference index would slot in behind the same seam without
/// touching the policies.
pub trait ReferenceLookup {
    /// The item-level definition name whose extent starts at byte offset
    /// `offset` in `file_id`, if the source records one (the lens join:
    /// the command executes the item the lens anchored, and the lens
    /// anchor *is* the item-level definition's start).
    fn item_name_at(&self, file_id: FileId, offset: usize) -> Option<String>;

    /// Every recorded non-definition reference of the item-level name
    /// `name` across the analysed session. Order is not significant; the
    /// policy sorts. Dummy-span records the graph keeps for synthesized
    /// shapes are skipped by the production source.
    fn references_of(&self, name: &str) -> Vec<DefinitionLocation>;
}

/// Production [`ReferenceLookup`]: borrows the driver's shared
/// [`ReferenceGraph`].
///
/// Borrowing (not cloning) keeps the request allocation-free apart from
/// the returned vectors; the guard lives in the caller, exactly like the
/// code-lens resolve handler.
pub struct GraphReferenceLookup<'a> {
    graph: &'a ReferenceGraph,
}

impl<'a> GraphReferenceLookup<'a> {
    /// Reads reference facts out of `graph`.
    #[must_use]
    pub fn new(graph: &'a ReferenceGraph) -> Self {
        Self { graph }
    }
}

impl ReferenceLookup for GraphReferenceLookup<'_> {
    fn item_name_at(&self, file_id: FileId, offset: usize) -> Option<String> {
        // The same join the code-lens resolve performs: the start offset
        // identifies the item-level definition of this document.
        GraphLensSource::new(self.graph, file_id)
            .lens_items()
            .into_iter()
            .find(|item| item.lo == offset)
            .map(|item| item.name)
    }

    fn references_of(&self, name: &str) -> Vec<DefinitionLocation> {
        self.graph
            .find_references(name)
            .iter()
            .filter(|reference| !reference.is_definition && !reference.span.is_dummy())
            .map(|reference| DefinitionLocation {
                file_id: reference.file_id,
                span: reference.span,
            })
            .collect()
    }
}

/// Parse the resolve's argument payload: exactly two arguments — the
/// document URI as a JSON string, the lens range as a JSON object — or
/// nothing. Every other shape answers `None` (the caller surfaces
/// [`ExecuteCommandError::MalformedArguments`]).
#[must_use]
pub fn parse_show_references_args(arguments: &[Value]) -> Option<ShowReferencesArgs> {
    let [uri, range] = arguments else {
        return None;
    };
    Some(ShowReferencesArgs {
        uri: uri.as_str()?.to_string(),
        range: serde_json::from_value(range.clone()).ok()?,
    })
}

/// Reference-target policy: the non-definition records of `name`, ordered
/// by (file id, span start), duplicates collapsed, zero-width extents
/// skipped (an extent that cannot name a jump target must not be
/// answered as one — the same rule the other engines apply).
#[must_use]
pub fn compute_reference_targets(
    source: &dyn ReferenceLookup,
    name: &str,
) -> Vec<DefinitionLocation> {
    let mut kept: Vec<DefinitionLocation> = Vec::new();
    for location in source.references_of(name) {
        if location.span.lo == location.span.hi {
            continue;
        }
        if kept.contains(&location) {
            continue;
        }
        kept.push(location);
    }
    kept.sort_by_key(|location| {
        (
            location.file_id.to_raw(),
            location.span.lo.to_usize(),
            location.span.hi.to_usize(),
        )
    });
    kept
}

/// `workspace/executeCommand` request entry point (LSP-EXEC-CMD).
///
/// Resolution order mirrors `resolve_code_lens`: command id check →
/// argument payload parse → URI → open-file lookup → range start to byte
/// offset → the lens join against the shared reference graph (no
/// re-analysis) → the name-keyed reference walk → wire locations. Every
/// failure is a named [`ExecuteCommandError`] the router maps into a
/// JSON-RPC error; the success path answers the referenced locations as
/// the request's JSON value, empty for an uncalled item (the count the
/// resolve's title rendered is exactly what this walk re-derives).
pub fn execute_command(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &ExecuteCommandParams,
) -> Result<Vec<Location>, ExecuteCommandError> {
    if params.command != SHOW_REFERENCES_COMMAND {
        return Err(ExecuteCommandError::UnknownCommand(params.command.clone()));
    }
    let args = parse_show_references_args(&params.arguments)
        .ok_or(ExecuteCommandError::MalformedArguments)?;

    let path = Url::parse(&args.uri)
        .ok()
        .and_then(|url| url.to_file_path().ok())
        .ok_or(ExecuteCommandError::UnjoinableReference)?;
    let file_id = file_map
        .get_by_path(&path)
        .ok_or(ExecuteCommandError::UnjoinableReference)?;

    let (name, targets) = {
        let offset = {
            let source_maps = db.source_maps.read();
            let sm = source_maps
                .get(&file_id)
                .ok_or(ExecuteCommandError::UnjoinableReference)?;
            sm.line_col_to_offset(
                args.range.start.line as usize,
                args.range.start.character as usize,
            )
            .ok_or(ExecuteCommandError::UnjoinableReference)?
        };
        let graph = db.reference_graph.read();
        let lookup = GraphReferenceLookup::new(&graph);
        let name = lookup
            .item_name_at(file_id, offset)
            .ok_or(ExecuteCommandError::UnjoinableReference)?;
        let targets = compute_reference_targets(&lookup, &name);
        (name, targets)
    };

    // Each target maps independently (one unmappable span in a file whose
    // map is gone must not erase the good answers); an answer that maps
    // to nothing while the resolve's title promised a count is a failure,
    // not an empty success — the count and the answer must agree.
    let source_maps = db.source_maps.read();
    let mut locations = Vec::with_capacity(targets.len());
    for target in &targets {
        let Some(sm) = source_maps.get(&target.file_id) else {
            continue;
        };
        let Some(((start_line, start_col), (end_line, end_col))) =
            sm.span_to_position(target.span.lo.to_usize(), target.span.hi.to_usize())
        else {
            continue;
        };
        let Some(target_path) = file_map.path(target.file_id) else {
            continue;
        };
        let Some(target_uri) = Url::from_file_path(target_path).ok() else {
            continue;
        };
        let Ok(uri) = lsp_types::Uri::from_str(target_uri.as_str()) else {
            continue;
        };
        locations.push(Location {
            uri,
            range: Range {
                start: lsp_types::Position {
                    line: start_line as u32,
                    character: start_col as u32,
                },
                end: lsp_types::Position {
                    line: end_line as u32,
                    character: end_col as u32,
                },
            },
        });
    }
    if targets.len() == locations.len() {
        tracing::debug!(name = %name, locations = locations.len(), "showReferences executed");
        return Ok(locations);
    }
    Err(ExecuteCommandError::UnjoinableReference)
}
