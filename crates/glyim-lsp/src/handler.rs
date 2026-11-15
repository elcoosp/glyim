use crate::AnalysisDatabase;
use crate::code_action::provide_code_actions;
use crate::completion::provide_completions;
use crate::document_highlight::document_highlights;
use crate::driver::AnalysisMessage;
use crate::folding::provide_folding_ranges;
use crate::formatting::format_document;
use crate::goto_definition::goto_definition;
use crate::hover::provide_hover;
use crate::navigation::{document_symbols, find_references, workspace_symbols};
use crate::prepare_rename::prepare_rename;
use crate::rename::rename_symbol;
use async_lsp::router::Router;
use std::ops::ControlFlow;

use lsp_types::notification::{DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentHighlightRequest, DocumentSymbolRequest,
    FoldingRangeRequest, Formatting, GotoDefinition, HoverRequest, Initialize,
    PrepareRenameRequest, References, Rename, Shutdown, WorkspaceSymbolRequest,
};
use lsp_types::*;
use std::sync::Arc;
use tokio::sync::mpsc;

/// `build_router`.
pub fn build_router(
    db: Arc<AnalysisDatabase>,
    _analysis_tx: mpsc::Sender<AnalysisMessage>,
    _client: async_lsp::ClientSocket,
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
                // LSP-HIGHLIGHT: the document-highlight capability. The
                // handler below reads the same reference graph as
                // find-references/rename; the conformance test pins this
                // advertisement to the router registration.
                document_highlight_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
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
                ..ServerCapabilities::default()
            };
            Ok(InitializeResult {
                capabilities,
                server_info: None,
            })
        }
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

    // Goto Definition
    let db_def = db.clone();
    router.request::<GotoDefinition, _>(move |(), params: GotoDefinitionParams| {
        let db = db_def.clone();
        async move {
            let guard = db.file_map.read();
            Ok(goto_definition(&db, &guard, &params))
        }
    });

    // Find References
    let db_ref = db.clone();
    router.request::<References, _>(move |(), params: ReferenceParams| {
        let db = db_ref.clone();
        async move {
            let guard = db.file_map.read();
            Ok(find_references(&db, &guard, &params))
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

    // Formatting
    let db_fmt = db.clone();
    router.request::<Formatting, _>(move |(), params: DocumentFormattingParams| {
        let db = db_fmt.clone();
        async move {
            let _guard = db.file_map.read();
            Ok(format_document(&db, &params))
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

    // Workspace Symbols (LSP-CONFORMANCE: was dead — advertised nothing,
    // routed nothing; the function and its tests existed but no client could
    // reach it).
    let db_ws = db.clone();
    router.request::<WorkspaceSymbolRequest, _>(move |(), params: WorkspaceSymbolParams| {
        let db = db_ws.clone();
        async move {
            Ok(Some(WorkspaceSymbolResponse::Flat(
                workspace_symbols(&db, &params).unwrap_or_default(),
            )))
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
    let tx_open = _analysis_tx.clone();
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
    let tx_change = _analysis_tx.clone();
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
    let tx_close = _analysis_tx.clone();
    router.notification::<DidCloseTextDocument>(move |(), params: DidCloseTextDocumentParams| {
        let path = path_from_uri(&params.text_document.uri);
        let () = db_close.file_map.write().remove(&path);
        // Same bounded-channel discipline as didOpen above.
        if let Err(err) = tx_close.try_send(AnalysisMessage::FileClosed { path: path.clone() }) {
            tracing::warn!(?path, %err, "analysis channel dropped the didClose notice");
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
