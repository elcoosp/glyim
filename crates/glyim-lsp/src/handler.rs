use crate::AnalysisDatabase;
use crate::call_hierarchy::{incoming_calls, outgoing_calls, prepare_call_hierarchy};
use crate::code_action::provide_code_actions;
use crate::code_lens::{code_lenses, resolve_code_lens};
use crate::completion::provide_completions;
use crate::document_highlight::document_highlights;
use crate::document_link::document_links;
use crate::driver::AnalysisMessage;
use crate::driver::warn_dropped_send;
use crate::execute_command::execute_command;
use crate::file_operations::{created_file_decisions, deleted_file_decisions};
use crate::file_rename::renamed_file_decisions;
use crate::folding::provide_folding_ranges;
use crate::formatting::format_document;
use crate::goto_declaration::goto_declaration;
use crate::goto_definition::goto_definition;
use crate::hover::provide_hover;
use crate::implementation::goto_implementation;
use crate::inlay_hint::inlay_hints;
use crate::inline_value::inline_values;
use crate::linked_editing::linked_editing_ranges;
use crate::moniker::monikers;
use crate::navigation::{document_symbols, find_references, workspace_symbols};
use crate::on_type_formatting::on_type_formatting;
use crate::prepare_rename::prepare_rename;
use crate::pull_diagnostics::pull_diagnostics;
use crate::range_formatting::range_formatting;
use crate::rename::rename_symbol;
use crate::selection_range::selection_ranges;
use crate::semantic_tokens::{semantic_tokens_full, semantic_tokens_range, token_legend};
use crate::signature_help::signature_help;
use crate::type_definition::type_definition;
use crate::type_hierarchy::{prepare_type_hierarchy, type_subtypes, type_supertypes};
use crate::watched_files::watched_file_decisions;
use crate::will_save::will_save_edits;
use crate::workspace_diagnostics::workspace_diagnostics;
use async_lsp::ResponseError;
use async_lsp::router::Router;
use std::ops::ControlFlow;

use lsp_types::notification::WillSaveTextDocument;
use lsp_types::notification::{
    DidChangeTextDocument, DidChangeWatchedFiles, DidCloseTextDocument, DidCreateFiles,
    DidDeleteFiles, DidOpenTextDocument, DidRenameFiles,
};
use lsp_types::request::{
    CallHierarchyIncomingCalls, CallHierarchyOutgoingCalls, CallHierarchyPrepare,
    CodeActionRequest, CodeLensRequest, CodeLensResolve, Completion, DocumentDiagnosticRequest,
    DocumentHighlightRequest, DocumentLinkRequest, DocumentSymbolRequest, ExecuteCommand,
    FoldingRangeRequest, Formatting, GotoDeclaration, GotoDefinition, GotoImplementation,
    GotoImplementationParams, GotoTypeDefinition, HoverRequest, Initialize, InlayHintRequest,
    InlineValueRequest, LinkedEditingRange, MonikerRequest, OnTypeFormatting, PrepareRenameRequest,
    RangeFormatting, References, Rename, SelectionRangeRequest, SemanticTokensFullRequest,
    SemanticTokensRangeRequest, Shutdown, SignatureHelpRequest, TypeHierarchyPrepare,
    TypeHierarchySubtypes, TypeHierarchySupertypes, WillSaveWaitUntil, WorkspaceDiagnosticRequest,
    WorkspaceSymbolRequest,
};
use lsp_types::*;
use std::sync::Arc;
use tokio::sync::mpsc;

/// The command ids the server advertises for `workspace/executeCommand`
/// (LSP-EXEC-CMD): the code-lens resolve's command id, and nothing else.
/// One source of truth — the capability advertisement below renders this
/// list, and the conformance test pins the resolve's emitted command id
/// against it.
pub fn advertised_commands() -> Vec<String> {
    vec![crate::code_lens::SHOW_REFERENCES_COMMAND.to_string()]
}

/// `build_router`.
pub fn build_router(
    db: Arc<AnalysisDatabase>,
    analysis_tx: mpsc::Sender<AnalysisMessage>,
) -> Router<()> {
    let mut router = Router::new(());
    // INF-16: the request handlers read the DRIVER's `db.file_map`
    // (populated by didOpen/didChange), not a throwaway local map.

    // Initialize
    let db_init = db.clone();
    router.request::<Initialize, _>(move |(), _params: InitializeParams| {
        let _db = db_init.clone();
        async move {
            let capabilities = ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        // LSP-WILL-SAVE: the save-time hooks. `will_save`
                        // turns the `textDocument/willSave` notification on
                        // (routed below, observability only); `will_save_
                        // wait_until` turns the request on — the handler
                        // below feeds the stored text through the same
                        // `format_source` core the full-document request
                        // reads (one formatter, three wire shapes), so a
                        // saved file and a manually formatted file agree by
                        // construction. The conformance test pins this
                        // advertisement to the router registration.
                        will_save: Some(true),
                        will_save_wait_until: Some(true),
                        ..Default::default()
                    },
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                // LSP-DECLARATION: the goto-declaration capability. The
                // handler below walks the parse trees the analysis driver
                // stores in `db.syntaxes` for `use` declarations and the
                // same symbol index goto-definition reads, so no
                // re-analysis happens on the request path. The conformance
                // test pins this advertisement to the router registration.
                declaration_provider: Some(DeclarationCapability::Simple(true)),
                // LSP-TYPE-DEF: the type-definition capability. The handler
                // below reads the Tier 6.4 typeck results through
                // `db.type_at_offset` and lands on the struct/enum the
                // cursor's expression carries; the conformance test pins
                // this advertisement to the router registration.
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                // LSP-HIGHLIGHT: the document-highlight capability. The
                // handler below reads the same reference graph as
                // find-references/rename; the conformance test pins this
                // advertisement to the router registration.
                document_highlight_provider: Some(OneOf::Left(true)),
                // LSP-SELECTION: the selection-range capability. The
                // handler below walks the parse trees the analysis driver
                // now stores in `db.syntaxes`; the conformance test pins
                // this advertisement to the router registration.
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                // LSP-SEMANTIC-TOKENS: the semantic-tokens capability. The
                // legend below is the same `token_legend()` the response
                // indices are pinned against (one source of truth), and the
                // handler walks the same stored parse trees as
                // selection-range. The conformance test pins this
                // advertisement to the router registration.
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                            legend: token_legend(),
                            // LSP-SEMANTIC-TOKENS: both shapes are served —
                            // full (startup / full refresh) and range
                            // (per-scroll), each with its own router entry.
                            range: Some(true),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                // LSP-CALL-HIERARCHY: the call-hierarchy capability. The
                // three handlers below read the same reference graph as
                // find-references/rename (its `ReferenceKind::Call` records
                // carry the owning item id), so no re-analysis happens on
                // the request path. The conformance test pins this
                // advertisement to the router registration.
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                // LSP-SIGNATURE-HELP: the signature-help capability. The
                // handler below reads the same reference graph the
                // call-hierarchy requests read (call records carry the
                // callee name and the whole call extent) and renders the
                // same symbol-index signatures hover renders. `(` and `,`
                // are the editor trigger characters; the conformance test
                // pins this advertisement to the router registration.
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    ..Default::default()
                }),
                // LSP-INLAY-HINT: the inlay-hint capability. The handler
                // below reads the same Tier 6.4 HIR + typeck state the
                // type-definition request reads (un-annotated `let`
                // bindings and their initializer types), so no
                // re-analysis happens on the request path. The
                // conformance test pins this advertisement to the router
                // registration.
                inlay_hint_provider: Some(OneOf::Left(true)),
                // LSP-MONIKER: the moniker capability. The handler below
                // reads the same shared `SymbolIndex` goto-definition and
                // hover read (recorded definitions with their qualified
                // module paths) plus the stored parse trees for the spelled
                // import paths, so no re-analysis happens on the request
                // path. The conformance test pins this advertisement to the
                // router registration.
                moniker_provider: Some(OneOf::Left(true)),
                // LSP-INLINE-VALUE: the inline-value capability. The
                // handler below reads the same stored HIRs the
                // type-hierarchy engine reads (every `let` binding with
                // its byte-exact pattern span — the same spans rename
                // edits consume), so no re-analysis happens on the request
                // path. The conformance test pins this advertisement to
                // the router registration.
                inline_value_provider: Some(OneOf::Left(true)),
                // LSP-IMPLEMENTATION: the goto-implementation capability.
                // The handler below walks the driver's stored `CrateHir`s
                // (each `impl` block records its trait path and self type
                // with byte-exact spans), so no re-analysis happens on the
                // request path. The conformance test pins this
                // advertisement to the router registration.
                implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
                // LSP-PULL-DIAG: the pull-diagnostics capability. The
                // handler below reads the same recorded set the push
                // path (`publishDiagnostics`) publishes, so a client
                // mixing both paths sees identical coordinates. The
                // language has inter-file dependencies (the driver
                // re-analyses dependents).
                // LSP-WORKSPACE-DIAG: `workspace_diagnostics` moved to
                // `true` alongside the `workspace/diagnostic` route
                // below — the workspace pull composes the same recorded
                // sets into one answer per analysed file, so the
                // capability now advertises a route that exists. The
                // conformance test pins this advertisement to the
                // router registration.
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions {
                        identifier: None,
                        inter_file_dependencies: true,
                        workspace_diagnostics: true,
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                    },
                )),
                references_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                // LSP-RANGE-FORMAT: the range-formatting capability. The
                // handler below feeds the claimed line span through the
                // same `format_source` core the full-document request
                // reads (one formatter, two wire shapes), seeded with the
                // brace balance of the unselected prefix so the edit
                // composes with the surrounding file. The conformance test
                // pins this advertisement to the router registration.
                document_range_formatting_provider: Some(OneOf::Left(true)),
                // LSP-PREPARE-RENAME: switch the plain-boolean rename
                // capability to options advertising `prepareProvider`, so
                // clients consult `textDocument/prepareRename` before
                // opening the rename input. The handler below shares the
                // occurrence seam with document highlight.
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                // LSP-CONFORMANCE: `navigation::workspace_symbols` existed
                // and was tested, but neither advertised nor routed — a dead
                // feature. Advertise it so clients send the request.
                workspace_symbol_provider: Some(OneOf::Left(true)),
                // LSP-DOC-LINK: the document-link capability. The handler
                // below walks the driver's stored parse trees (the same
                // immutable green store selection-range and semantic-tokens
                // read) for include-macro call shapes, so no re-analysis
                // happens on the request path and the capability answers on
                // broken code too. Targets resolve directly — no
                // `documentLink/resolve` round-trip — so the resolve
                // provider stays off. The conformance test pins this
                // advertisement to the router registration.
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
                // LSP-LINKED-EDIT: the linked-editing-range capability. The
                // handler below composes the document-highlight occurrence
                // seam (the shared reference graph, T129 scope-owner
                // filter included), so multi-cursor in-place editing follows
                // exactly the same scoping rules highlight, find-references
                // and rename enforce. The conformance test pins this
                // advertisement to the router registration.
                linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(
                    true,
                )),
                // LSP-CODE-LENS: the code-lens capability. The two handlers
                // below read the same shared reference graph
                // find-references/rename read (item-level definitions,
                // name-keyed use counts), so no re-analysis happens on the
                // request path; the resolve re-derives its count at resolve
                // time. The conformance test pins this advertisement to the
                // router registration.
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(true),
                }),
                // LSP-ON-TYPE-FORMAT: the on-type-formatting capability.
                // The handler below walks the driver's stored parse trees
                // (the same immutable green store document-link and
                // selection-range read) for the matching opener of a
                // just-typed closing bracket, so no re-analysis happens
                // on the request path and the fix answers on broken code
                // too. `}` is the first trigger; `)` and `]` join it —
                // the three closing brackets the policy can pair. The
                // conformance test pins this advertisement to the router
                // registration.
                document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
                    first_trigger_character: "}".to_string(),
                    more_trigger_character: Some(vec![")".to_string(), "]".to_string()]),
                }),
                // LSP-EXEC-CMD: the execute-command capability. The
                // advertised set is exactly the command the code-lens
                // resolve emits (`glyim.showReferences`, LSP-CODE-LENS) —
                // before this route existed, the resolve attached a
                // command no server route could execute, the exact
                // "advertised but unreachable" shape the conformance
                // audit exists for. The handler below serves it from the
                // same shared reference graph the resolve counts, so the
                // executed locations agree with the rendered title by
                // construction. `advertised_commands()` is the single
                // source of truth; the conformance test pins the
                // advertisement to the resolve's emitted id.
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: advertised_commands(),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
                // LSP-FILE-RENAME: the file-rename capability. The
                // handler below turns each client-reported rename into
                // the per-role session decision the engine computes —
                // editor-owned documents re-key in place (the `FileId`
                // and every fact keyed by it survive), watched loads
                // follow the watched-files honest refresh — so the
                // session's path-keyed state follows the file instead of
                // pointing at a path that no longer exists. The
                // `**/*.g` filter names the only files the session
                // records facts for. The conformance test pins this
                // advertisement to the router registration.
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: None,
                    file_operations: Some(WorkspaceFileOperationsServerCapabilities {
                        did_rename: Some(FileOperationRegistrationOptions {
                            filters: vec![FileOperationFilter {
                                pattern: FileOperationPattern {
                                    glob: "**/*.g".to_string(),
                                    matches: None,
                                    options: None,
                                },
                                scheme: None,
                            }],
                        }),
                        // LSP-FILE-CREATE-DELETE: the declared
                        // counterparts of the watched events — the
                        // routes below run the same ownership policy
                        // the watched route runs, over the operations
                        // the client's workspace UI performed. The
                        // conformance test pins the advertisement to
                        // the router registrations.
                        did_create: Some(FileOperationRegistrationOptions {
                            filters: vec![FileOperationFilter {
                                pattern: FileOperationPattern {
                                    glob: "**/*.g".to_string(),
                                    matches: None,
                                    options: None,
                                },
                                scheme: None,
                            }],
                        }),
                        will_create: None,
                        will_rename: None,
                        did_delete: Some(FileOperationRegistrationOptions {
                            filters: vec![FileOperationFilter {
                                pattern: FileOperationPattern {
                                    glob: "**/*.g".to_string(),
                                    matches: None,
                                    options: None,
                                },
                                scheme: None,
                            }],
                        }),
                        will_delete: None,
                    }),
                }),
                ..ServerCapabilities::default()
            };
            Ok(InitializeResult {
                capabilities,
                server_info: None,
            })
        }
    });

    // Save-time edits (LSP-WILL-SAVE): the client asks for the edits that
    // bring the document to its saved form before it hits the disk — the
    // format-on-save hook. The handler feeds the stored text through the
    // same `format_source` core the full-document request reads; an
    // already-formatted document answers `null` (no edit churn), and the
    // save reason is deliberately not consulted (the formatter is
    // reason-agnostic).
    let db_will_save = db.clone();
    router.request::<WillSaveWaitUntil, _>(move |(), params: WillSaveTextDocumentParams| {
        let db = db_will_save.clone();
        async move {
            let guard = db.file_map.read();
            Ok(will_save_edits(&db, &guard, &params))
        }
    });

    // Will Save (LSP-WILL-SAVE): the notification half of the save
    // negotiation. It carries no request semantics — clients that
    // negotiated `willSaveWaitUntil` send the request separately — so the
    // route records the event for the log file and moves on.
    router.notification::<WillSaveTextDocument>(move |(), params: WillSaveTextDocumentParams| {
        tracing::debug!(
            uri = %params.text_document.uri.as_str(),
            "textDocument/willSave received"
        );
        ControlFlow::Continue(())
    });

    // Shutdown
    router.request::<Shutdown, _>(move |(), (): ()| async move { Ok(()) });

    // Completion
    let db_comp = db.clone();
    router.request::<Completion, _>(move |(), params: CompletionParams| {
        let db = db_comp.clone();
        async move {
            let guard = db.file_map.read();
            Ok(provide_completions(&db, &guard, &params))
        }
    });

    // Hover
    let db_hover = db.clone();
    router.request::<HoverRequest, _>(move |(), params: HoverParams| {
        let db = db_hover.clone();
        async move {
            let guard = db.file_map.read();
            Ok(provide_hover(&db, &guard, &params))
        }
    });

    // Goto Declaration
    let db_def = db.clone();
    router.request::<GotoDefinition, _>(move |(), params: GotoDefinitionParams| {
        let db = db_def.clone();
        async move {
            let guard = db.file_map.read();
            Ok(goto_definition(&db, &guard, &params))
        }
    });

    // Goto Declaration (LSP-DECLARATION): same params shape as
    // goto-definition, but the target is the *declaration* — the `use`
    // statement that binds the cursor's name in this file, or the local
    // item that declares it. A name neither imported nor declared here
    // answers `null` instead of double-serving goto-definition.
    let db_decl = db.clone();
    router.request::<GotoDeclaration, _>(
        move |(), params: lsp_types::request::GotoDeclarationParams| {
            let db = db_decl.clone();
            async move {
                let guard = db.file_map.read();
                Ok(goto_declaration(&db, &guard, &params))
            }
        },
    );

    // Goto Type Definition (LSP-TYPE-DEF): same params/response shape as
    // goto-definition, but the target is the *shape* — the struct or enum
    // the expression under the cursor resolves to.
    let db_ty_def = db.clone();
    router.request::<GotoTypeDefinition, _>(move |(), params: GotoDefinitionParams| {
        let db = db_ty_def.clone();
        async move {
            let guard = db.file_map.read();
            Ok(type_definition(&db, &guard, &params))
        }
    });

    // Goto Implementation (LSP-IMPLEMENTATION): same params shape as the
    // other goto requests, but the target is the *implementations* — the
    // `impl` blocks whose trait path or self type names the cursor's
    // symbol, across every file the session analysed. Multiple impls
    // answer an array in (file, offset) order; none answers `null`.
    let db_impl = db.clone();
    router.request::<GotoImplementation, _>(move |(), params: GotoImplementationParams| {
        let db = db_impl.clone();
        async move {
            let guard = db.file_map.read();
            Ok(goto_implementation(&db, &guard, &params))
        }
    });

    // Pull Diagnostics (LSP-PULL-DIAG): the same recorded set the push
    // path publishes, answered as a full report on demand — a clean
    // analysed file answers an empty report. A document the server does
    // not know (not open, never analysed) answers a JSON-RPC error: the
    // pull result type is a report, not an optional one, so `null` is not
    // a legal answer on the wire.
    let db_pull = db.clone();
    router.request::<DocumentDiagnosticRequest, _>(move |(), params: DocumentDiagnosticParams| {
        let db = db_pull.clone();
        async move {
            let guard = db.file_map.read();
            match pull_diagnostics(&db, &guard, &params) {
                Some(report) => Ok(report),
                None => Err(ResponseError::new(
                    async_lsp::ErrorCode::INVALID_PARAMS,
                    "textDocument/diagnostic: document not open or never analysed",
                )),
            }
        }
    });

    // Workspace Diagnostics (LSP-WORKSPACE-DIAG): one full report per
    // analysed file, composed from the same recorded sets the push path
    // publishes and the per-file pull answers. The pull is always
    // answerable — an empty session answers an empty `items` array, and
    // the params' echoed `previous_result_ids` are deliberately ignored
    // (this server never issues result ids) — so unlike the per-file
    // pull there is no JSON-RPC error case here.
    let db_ws_diag = db.clone();
    router.request::<WorkspaceDiagnosticRequest, _>(
        move |(), params: WorkspaceDiagnosticParams| {
            let db = db_ws_diag.clone();
            async move {
                let guard = db.file_map.read();
                Ok(workspace_diagnostics(&db, &guard, &params))
            }
        },
    );

    // Find References
    let db_ref = db.clone();
    router.request::<References, _>(move |(), params: ReferenceParams| {
        let db = db_ref.clone();
        async move {
            let guard = db.file_map.read();
            Ok(find_references(&db, &guard, &params))
        }
    });

    // Document Links (LSP-DOC-LINK): the string literals of the open
    // document's include-macro calls become clickable file links — the
    // whole `include!`/`include_str!`/`include_bytes!("…")` shape links to
    // the path it spells, resolved against the containing document's
    // directory. The walk reads the stored parse trees (no re-analysis),
    // so it answers on broken code too; no include calls answer `null`.
    let db_doc_link = db.clone();
    router.request::<DocumentLinkRequest, _>(move |(), params: DocumentLinkParams| {
        let db = db_doc_link.clone();
        async move {
            let guard = db.file_map.read();
            Ok(document_links(&db, &guard, &params))
        }
    });

    // Linked Editing Ranges (LSP-LINKED-EDIT): the cursor's symbol becomes
    // an in-place multi-cursor edit group — every same-file occurrence the
    // document-highlight policy vouches for (shared reference graph, T129
    // scope-owner filter included) answers as one range group, kinds
    // dropped. Off-symbol cursors and unopened documents answer `null`.
    let db_linked_edit = db.clone();
    router.request::<LinkedEditingRange, _>(move |(), params: LinkedEditingRangeParams| {
        let db = db_linked_edit.clone();
        async move {
            let guard = db.file_map.read();
            Ok(linked_editing_ranges(&db, &guard, &params))
        }
    });

    // Call Hierarchy (LSP-CALL-HIERARCHY): prepare answers the function
    // item under the cursor (definition site, or a call site of the
    // callee); incoming/outgoing re-resolve the client's echoed item by
    // its definition extent and walk the reference graph's call records.
    let db_prepare_ch = db.clone();
    router.request::<CallHierarchyPrepare, _>(move |(), params: CallHierarchyPrepareParams| {
        let db = db_prepare_ch.clone();
        async move {
            let guard = db.file_map.read();
            Ok(prepare_call_hierarchy(&db, &guard, &params))
        }
    });

    let db_incoming = db.clone();
    router.request::<CallHierarchyIncomingCalls, _>(
        move |(), params: CallHierarchyIncomingCallsParams| {
            let db = db_incoming.clone();
            async move {
                let guard = db.file_map.read();
                Ok(incoming_calls(&db, &guard, &params))
            }
        },
    );

    let db_outgoing = db.clone();
    router.request::<CallHierarchyOutgoingCalls, _>(
        move |(), params: CallHierarchyOutgoingCallsParams| {
            let db = db_outgoing.clone();
            async move {
                let guard = db.file_map.read();
                Ok(outgoing_calls(&db, &guard, &params))
            }
        },
    );

    // Type Hierarchy (LSP-TYPE-HIERARCHY): prepare answers the
    // trait/struct/enum item under the cursor (definition site, whole
    // item extent); supertypes/subtypes re-resolve the client's echoed
    // item by its definition extent and walk the recorded impl facts.
    // lsp-types 0.97 carries no ServerCapabilities field for these
    // requests (the LSP spec gates them on registration, not on a static
    // capability), so the router registration *is* the advertisement —
    // pinned by the conformance and end-to-end tests.
    let db_prepare_th = db.clone();
    router.request::<TypeHierarchyPrepare, _>(move |(), params: TypeHierarchyPrepareParams| {
        let db = db_prepare_th.clone();
        async move {
            let guard = db.file_map.read();
            Ok(prepare_type_hierarchy(&db, &guard, &params))
        }
    });

    let db_supertypes = db.clone();
    router.request::<TypeHierarchySupertypes, _>(
        move |(), params: TypeHierarchySupertypesParams| {
            let db = db_supertypes.clone();
            async move {
                let guard = db.file_map.read();
                Ok(type_supertypes(&db, &guard, &params))
            }
        },
    );

    let db_subtypes = db.clone();
    router.request::<TypeHierarchySubtypes, _>(move |(), params: TypeHierarchySubtypesParams| {
        let db = db_subtypes.clone();
        async move {
            let guard = db.file_map.read();
            Ok(type_subtypes(&db, &guard, &params))
        }
    });

    // Inline Value (LSP-INLINE-VALUE): the recorded `let` bindings of the
    // requested range answer as variable lookups — the facts a debug
    // adapter stopped on a frame needs to render inline variable names,
    // served from the stored HIRs (the same spans rename edits consume).
    let db_inline_values = db.clone();
    router.request::<InlineValueRequest, _>(move |(), params: InlineValueParams| {
        let db = db_inline_values.clone();
        async move {
            let guard = db.file_map.read();
            Ok(inline_values(&db, &guard, &params))
        }
    });

    // Moniker (LSP-MONIKER): the cursor's identifier resolves to the
    // identities the recorded facts vouch for — the spelled path of the
    // `use` binding it sits on, the qualified module path of the definition
    // it sits on, or the name-only resolution of a bare usage. lsp-types
    // 0.97 carries the `moniker_provider` capability, advertised above and
    // pinned by the conformance test.
    let db_moniker = db.clone();
    router.request::<MonikerRequest, _>(move |(), params: MonikerParams| {
        let db = db_moniker.clone();
        async move {
            let guard = db.file_map.read();
            Ok(monikers(&db, &guard, &params))
        }
    });

    // Signature Help (LSP-SIGNATURE-HELP): the innermost recorded call
    // containing the cursor resolves through the reference graph; the
    // callee's signature renders from the symbol index, and the active
    // parameter is counted from the source text.
    let db_sig = db.clone();
    router.request::<SignatureHelpRequest, _>(move |(), params: SignatureHelpParams| {
        let db = db_sig.clone();
        async move {
            let guard = db.file_map.read();
            Ok(signature_help(&db, &guard, &params))
        }
    });

    // Inlay Hints (LSP-INLAY-HINT): un-annotated `let` bindings answer
    // `: Ty` inline, resolved from the same Tier 6.4 HIR + typeck state
    // the type-definition request reads — a walk of recorded facts, no
    // re-analysis on the request path. Annotated bindings, types with
    // no stable user-facing spelling, and bindings outside the
    // requested range are skipped honestly.
    let db_inlay = db.clone();
    router.request::<InlayHintRequest, _>(move |(), params: InlayHintParams| {
        let db = db_inlay.clone();
        async move {
            let guard = db.file_map.read();
            Ok(inlay_hints(&db, &guard, &params))
        }
    });

    // Document Highlight (LSP-HIGHLIGHT)
    let db_highlight = db.clone();
    router.request::<DocumentHighlightRequest, _>(move |(), params: DocumentHighlightParams| {
        let db = db_highlight.clone();
        async move {
            let guard = db.file_map.read();
            Ok(document_highlights(&db, &guard, &params))
        }
    });

    // Selection Range (LSP-SELECTION): walks the driver's stored parse
    // trees, so expand-selection follows the real token → statement →
    // item → file nesting instead of character guesses.
    let db_selection = db.clone();
    router.request::<SelectionRangeRequest, _>(move |(), params: SelectionRangeParams| {
        let db = db_selection.clone();
        async move {
            let guard = db.file_map.read();
            Ok(selection_ranges(&db, &guard, &params))
        }
    });

    // Semantic Tokens (LSP-SEMANTIC-TOKENS): full-document token
    // classification over the driver's stored parse trees, delta-encoded
    // against the same `token_legend()` the capability advertises.
    let db_semantic = db.clone();
    router.request::<SemanticTokensFullRequest, _>(move |(), params: SemanticTokensParams| {
        let db = db_semantic.clone();
        async move {
            let guard = db.file_map.read();
            Ok(semantic_tokens_full(&db, &guard, &params))
        }
    });

    // Semantic Tokens range (LSP-SEMANTIC-TOKENS): the same engine
    // restricted to the tokens intersecting a screen range — the request
    // editors fire while scrolling.
    let db_semantic_range = db.clone();
    router.request::<SemanticTokensRangeRequest, _>(
        move |(), params: SemanticTokensRangeParams| {
            let db = db_semantic_range.clone();
            async move {
                let guard = db.file_map.read();
                Ok(semantic_tokens_range(&db, &guard, &params))
            }
        },
    );

    // Formatting
    let db_fmt = db.clone();
    router.request::<Formatting, _>(move |(), params: DocumentFormattingParams| {
        let db = db_fmt.clone();
        async move {
            // No outer `file_map` guard here: `format_document` takes its
            // own read lock for the lookup it performs; a second one held
            // across the whole request would be a dead lock acquisition.
            Ok(format_document(&db, &params))
        }
    });

    // Range Formatting (LSP-RANGE-FORMAT): the claimed line span of the
    // selection re-indents through the same shared formatter core the
    // full-document request reads, seeded with the unselected prefix's
    // brace balance. Already-formatted selections answer `null` — never
    // an edit with no effect.
    let db_range_fmt = db.clone();
    router.request::<RangeFormatting, _>(move |(), params: DocumentRangeFormattingParams| {
        let db = db_range_fmt.clone();
        async move {
            let guard = db.file_map.read();
            Ok(range_formatting(&db, &guard, &params))
        }
    });

    // On-Type Formatting (LSP-ON-TYPE-FORMAT): a just-typed closing
    // bracket gets one whitespace-run edit placing its line at the
    // matching opener's indentation. The walk reads the stored parse
    // trees (no re-analysis); unbalanced shapes, family mismatches,
    // openers and already-correct indents answer `null` — never a guess.
    let db_on_type = db.clone();
    router.request::<OnTypeFormatting, _>(move |(), params: DocumentOnTypeFormattingParams| {
        let db = db_on_type.clone();
        async move {
            let guard = db.file_map.read();
            Ok(on_type_formatting(&db, &guard, &params))
        }
    });

    // Rename
    let db_rename = db.clone();
    router.request::<Rename, _>(move |(), params: RenameParams| {
        let db = db_rename.clone();
        async move {
            let guard = db.file_map.read();
            Ok(rename_symbol(&db, &guard, &params))
        }
    });

    // Prepare Rename (LSP-PREPARE-RENAME): `PrepareRenameRequest` takes the
    // bare `TextDocumentPositionParams` per the LSP spec.
    let db_prepare = db.clone();
    router.request::<PrepareRenameRequest, _>(move |(), params: TextDocumentPositionParams| {
        let db = db_prepare.clone();
        async move {
            let guard = db.file_map.read();
            Ok(prepare_rename(&db, &guard, &params))
        }
    });

    // FoldingRange - using FoldingRangeRequest
    let db_fold = db.clone();
    router.request::<FoldingRangeRequest, _>(move |(), params: FoldingRangeParams| {
        let db = db_fold.clone();
        async move { Ok(provide_folding_ranges(&db, &params)) }
    });

    // Code Action
    let db_action = db.clone();
    router.request::<CodeActionRequest, _>(move |(), params: CodeActionParams| {
        let db = db_action.clone();
        async move {
            let guard = db.file_map.read();
            Ok(provide_code_actions(&db, &guard, &params))
        }
    });

    // Code Lenses (LSP-CODE-LENS): one lens per item-level definition of
    // the open document, over the same shared reference graph
    // find-references and rename read. The response carries no commands —
    // the editor resolves each lens it is about to render; a document
    // that is not open or has no item-level definitions answers `null`.
    let db_lens = db.clone();
    router.request::<CodeLensRequest, _>(move |(), params: CodeLensParams| {
        let db = db_lens.clone();
        async move {
            let guard = db.file_map.read();
            Ok(code_lenses(&db, &guard, &params))
        }
    });

    // Code Lens Resolve (LSP-CODE-LENS): the echoed lens is re-joined
    // against the reference graph by definition extent (the call-hierarchy
    // join) and the live use count is rendered into the command title — a
    // stale or fabricated echo is a JSON-RPC error (the result type is a
    // plain `CodeLens`, not an optional one, so `null` is not a legal
    // answer on the wire), never a guessed count.
    let db_lens_resolve = db.clone();
    router.request::<CodeLensResolve, _>(move |(), params: CodeLens| {
        let db = db_lens_resolve.clone();
        async move {
            let guard = db.file_map.read();
            match resolve_code_lens(&db, &guard, &params) {
                Some(lens) => Ok(lens),
                None => Err(ResponseError::new(
                    async_lsp::ErrorCode::INVALID_PARAMS,
                    "codeLens/resolve: the echoed lens matches no live item-level definition",
                )),
            }
        }
    });

    // Execute Command (LSP-EXEC-CMD): the code-lens resolve's
    // `glyim.showReferences` command executes here. The echoed argument
    // payload re-joins against the shared reference graph by definition
    // extent (the resolve's own join) and answers the name-keyed
    // reference locations — the count the resolve's title rendered is
    // exactly what this walk re-derives. Every failure mode is a JSON-RPC
    // error naming the failed step; a never-advertised command id is
    // rejected before any fact is consulted.
    let db_exec = db.clone();
    router.request::<ExecuteCommand, _>(move |(), params: ExecuteCommandParams| {
        let db = db_exec.clone();
        async move {
            let guard = db.file_map.read();
            match execute_command(&db, &guard, &params) {
                Ok(locations) => {
                    // `Location` serializes to the wire shape the LSP spec
                    // defines; the conversion is total (no fallible
                    // maps) and cannot fail.
                    Ok(Some(serde_json::to_value(locations).map_err(|err| {
                        ResponseError::new(
                            async_lsp::ErrorCode::INTERNAL_ERROR,
                            format!(
                                "workspace/executeCommand: response serialization failed: {err}"
                            ),
                        )
                    })?))
                }
                Err(err) => Err(ResponseError::new(
                    async_lsp::ErrorCode::INVALID_PARAMS,
                    err.to_string(),
                )),
            }
        }
    });

    // Workspace Symbols (LSP-CONFORMANCE: was dead — advertised nothing,
    // routed nothing; the function and its tests existed but no client could
    // reach it). `workspace_symbols` answers `None` only for a session that
    // never indexed a workspace; the flat empty list is the honest
    // answerable-empty here (the workspace-diagnostics convention), not a
    // swallowed error — the symbol walk has no error case to hide.
    let db_ws = db.clone();
    router.request::<WorkspaceSymbolRequest, _>(move |(), params: WorkspaceSymbolParams| {
        let db = db_ws.clone();
        async move {
            let symbols = workspace_symbols(&db, &params).unwrap_or_default();
            Ok(Some(WorkspaceSymbolResponse::Flat(symbols)))
        }
    });

    // Document Symbols
    let db_doc = db.clone();
    router.request::<DocumentSymbolRequest, _>(move |(), params: DocumentSymbolParams| {
        let db = db_doc.clone();
        async move {
            let guard = db.file_map.read();
            Ok(document_symbols(&db, &guard, &params))
        }
    });

    // INF-16: document-sync notifications. Without these, nothing ever
    // populated the analysis `db.file_map`, so every request returned null.
    // The analysis driver is the single writer: didOpen/didChange forward the
    // full text (this server negotiates FULL sync) via `analysis_tx`.
    let db_open = db.clone();
    let tx_open = analysis_tx.clone();
    router.notification::<DidOpenTextDocument>(move |(), params: DidOpenTextDocumentParams| {
        let path = path_from_uri(&params.text_document.uri);
        let content = params.text_document.text;
        let version = params.text_document.version;
        let _ = db_open.file_map.write().get_or_create(&path);
        // The channel is bounded (16, see driver) and `try_send` fails when
        // it is full or the driver is gone; dropping the edit silently
        // would leave the session analysing stale text with no trace, so
        // record it (same discipline as `state::did_change`'s warn).
        if let Err(err) = tx_open.try_send(AnalysisMessage::FileChanged {
            path: path.clone(),
            content,
            version,
        }) {
            tracing::warn!(?path, %err, "analysis channel dropped the didOpen edit");
        }
        ControlFlow::Continue(())
    });

    let db_change = db.clone();
    let tx_change = analysis_tx.clone();
    router.notification::<DidChangeTextDocument>(move |(), params: DidChangeTextDocumentParams| {
        // FULL sync: the last content change carries the whole document.
        let path = path_from_uri(&params.text_document.uri);
        let version = params.text_document.version;
        if let Some(change) = params.content_changes.into_iter().last() {
            let _ = db_change.file_map.write().get_or_create(&path);
            // Same bounded-channel discipline as didOpen above.
            if let Err(err) = tx_change.try_send(AnalysisMessage::FileChanged {
                path: path.clone(),
                content: change.text,
                version,
            }) {
                tracing::warn!(?path, %err, "analysis channel dropped the didChange edit");
            }
        }
        ControlFlow::Continue(())
    });

    let db_close = db.clone();
    let tx_close = analysis_tx.clone();
    router.notification::<DidCloseTextDocument>(move |(), params: DidCloseTextDocumentParams| {
        let path = path_from_uri(&params.text_document.uri);
        let () = db_close.file_map.write().remove(&path);
        // Same bounded-channel discipline as didOpen above.
        if let Err(err) = tx_close.try_send(AnalysisMessage::FileClosed { path: path.clone() }) {
            tracing::warn!(?path, %err, "analysis channel dropped the didClose notice");
        }
        ControlFlow::Continue(())
    });

    // Watched Files (LSP-WATCHED-FILES): the client forwards the file
    // events for paths its watchers matched — the refresh path for
    // content no editor ever opened. Each decision executes here: a
    // re-analysis reads the file from disk and feeds the driver (the
    // path is recorded as watched first, so a later event stays
    // re-analysable; a read failure is a logged skip, never a panic —
    // one unreadable file must not erase the good decisions), a forget
    // reuses the driver's close cleanup (recorded facts cleared, empty
    // diagnostics published). lsp-types 0.97 carries no server
    // capability field for this notification, so the registration *is*
    // the advertisement — pinned by the wire and conformance tests.
    let db_watched_files = db.clone();
    let tx_watched_files = analysis_tx.clone();
    router.notification::<DidChangeWatchedFiles>(move |(), params: DidChangeWatchedFilesParams| {
        let decisions = watched_file_decisions(&db_watched_files, &params);
        for path in &decisions.reanalyse {
            db_watched_files.watched_files.write().insert(path.clone());
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    if let Err(err) =
                        tx_watched_files.try_send(AnalysisMessage::WatchedFileChanged {
                            path: path.clone(),
                            content,
                        })
                    {
                        warn_dropped_send(err);
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        %err,
                        "watched file unreadable; re-analysis skipped"
                    );
                }
            }
        }
        for path in &decisions.forget {
            db_watched_files.watched_files.write().remove(path);
            if let Err(err) =
                tx_watched_files.try_send(AnalysisMessage::FileClosed { path: path.clone() })
            {
                warn_dropped_send(err);
            }
        }
        ControlFlow::Continue(())
    });

    // File Create (LSP-FILE-CREATE-DELETE): the client declares the
    // file its workspace UI created. The decisions reuse the watched
    // policy on purpose — the declared shape is the forwarded events'
    // counterpart, and one ownership rule decides both — so a created
    // file no editor owns re-analyses from disk (its definitions may
    // resolve bare usages in the open documents) and a path the editor
    // owns is skipped (its lifecycle belongs to didOpen/didClose). The
    // execution mirrors the watched route exactly.
    let db_file_create = db.clone();
    let tx_file_create = analysis_tx.clone();
    router.notification::<DidCreateFiles>(move |(), params: CreateFilesParams| {
        let decisions = created_file_decisions(&db_file_create, &params);
        for path in &decisions.reanalyse {
            db_file_create.watched_files.write().insert(path.clone());
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    if let Err(err) = tx_file_create.try_send(AnalysisMessage::WatchedFileChanged {
                        path: path.clone(),
                        content,
                    }) {
                        warn_dropped_send(err);
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        %err,
                        "created file unreadable; re-analysis skipped"
                    );
                }
            }
        }
        ControlFlow::Continue(())
    });

    // File Delete (LSP-FILE-CREATE-DELETE): the client declares the
    // file its workspace UI deleted. A path no editor owns is forgotten
    // (the close cleanup clears the recorded facts and publishes an
    // empty report); a path the editor owns is skipped — didClose owns
    // the lifecycle. The execution mirrors the watched route exactly.
    let db_file_delete = db.clone();
    let tx_file_delete = analysis_tx.clone();
    router.notification::<DidDeleteFiles>(move |(), params: DeleteFilesParams| {
        let decisions = deleted_file_decisions(&db_file_delete, &params);
        for path in &decisions.forget {
            db_file_delete.watched_files.write().remove(path);
            if let Err(err) =
                tx_file_delete.try_send(AnalysisMessage::FileClosed { path: path.clone() })
            {
                warn_dropped_send(err);
            }
        }
        ControlFlow::Continue(())
    });

    // File Rename (LSP-FILE-RENAME): the client reports the file moves
    // its own UI drives — the session's path-keyed state must follow
    // them or goto-definition and find-references land on paths that no
    // longer exist. Each decision executes here: a re-key feeds the
    // driver's `FileRenamed` message (the path maps move, the `FileId`
    // and the facts keyed by it survive); a refresh is the watched-files
    // honest refresh over the new path — the old path is forgotten (the
    // close cleanup clears the recorded facts and publishes an empty
    // report) and the new one re-analyses from disk (a read failure is
    // a logged skip, never a panic — one unreadable file must not erase
    // the good decisions). The advertisement lives in the initialize
    // capabilities (`workspace.fileOperations.didRename`); the wire and
    // conformance tests pin the pair.
    let db_file_rename = db.clone();
    let tx_file_rename = analysis_tx.clone();
    router.notification::<DidRenameFiles>(move |(), params: RenameFilesParams| {
        let decisions = renamed_file_decisions(&db_file_rename, &params);
        for rename in &decisions.refresh {
            db_file_rename.watched_files.write().remove(&rename.old);
            db_file_rename
                .watched_files
                .write()
                .insert(rename.new.clone());
            if let Err(err) = tx_file_rename.try_send(AnalysisMessage::FileClosed {
                path: rename.old.clone(),
            }) {
                warn_dropped_send(err);
            }
            match std::fs::read_to_string(&rename.new) {
                Ok(content) => {
                    if let Err(err) = tx_file_rename.try_send(AnalysisMessage::WatchedFileChanged {
                        path: rename.new.clone(),
                        content,
                    }) {
                        warn_dropped_send(err);
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        path = %rename.new.display(),
                        %err,
                        "renamed file unreadable; re-analysis skipped"
                    );
                }
            }
        }
        for rename in &decisions.rekey {
            if let Err(err) = tx_file_rename.try_send(AnalysisMessage::FileRenamed {
                old: rename.old.clone(),
                new: rename.new.clone(),
            }) {
                warn_dropped_send(err);
            }
        }
        ControlFlow::Continue(())
    });

    router
}

/// Map an LSP document URI to a filesystem path (INF-16). Non-`file:` URIs
/// have no on-disk path; we fall back to the URI's path component.
fn path_from_uri(uri: &lsp_types::Uri) -> std::path::PathBuf {
    let s = uri.as_str();
    match url::Url::parse(s).ok().and_then(|u| u.to_file_path().ok()) {
        Some(p) => p,
        None => std::path::PathBuf::from(s),
    }
}
