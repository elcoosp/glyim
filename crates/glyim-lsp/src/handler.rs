use crate::AnalysisDatabase;
use crate::code_action::provide_code_actions;
use crate::completion::provide_completions;
use crate::driver::AnalysisMessage;
use crate::folding::provide_folding_ranges;
use crate::formatting::format_document;
use crate::goto_definition::goto_definition;
use crate::hover::provide_hover;
use crate::navigation::{document_symbols, find_references};
use crate::rename::rename_symbol;
use async_lsp::router::Router;
use std::ops::ControlFlow;

use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument,
};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentSymbolRequest, FoldingRangeRequest, Formatting,
    GotoDefinition, HoverRequest, Initialize, References, Rename, Shutdown,
};
use lsp_types::*;
use std::sync::Arc;
use tokio::sync::mpsc;

/// build_router.
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
    router.request::<Initialize, _>(move |_, _params: InitializeParams| {
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
                references_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            };
            Ok(InitializeResult {
                capabilities,
                server_info: None,
            })
        }
    });

    // Shutdown
    router.request::<Shutdown, _>(move |_, _: ()| async move { Ok(()) });

    // Completion
    let db_comp = db.clone();
    router.request::<Completion, _>(move |_, params: CompletionParams| {
        let db = db_comp.clone();
        async move {
            let guard = db.file_map.read();
            Ok(provide_completions(&db, &guard, &params))
        }
    });

    // Hover
    let db_hover = db.clone();
    router.request::<HoverRequest, _>(move |_, params: HoverParams| {
        let db = db_hover.clone();
        async move {
            let guard = db.file_map.read();
            Ok(provide_hover(&db, &guard, &params))
        }
    });

    // Goto Definition
    let db_def = db.clone();
    router.request::<GotoDefinition, _>(move |_, params: GotoDefinitionParams| {
        let db = db_def.clone();
        async move {
            let guard = db.file_map.read();
            Ok(goto_definition(&db, &guard, &params))
        }
    });

    // Find References
    let db_ref = db.clone();
    router.request::<References, _>(move |_, params: ReferenceParams| {
        let db = db_ref.clone();
        async move {
            let guard = db.file_map.read();
            Ok(find_references(&db, &guard, &params))
        }
    });

    // Formatting
    let db_fmt = db.clone();
    router.request::<Formatting, _>(move |_, params: DocumentFormattingParams| {
        let db = db_fmt.clone();
        async move {
            let _guard = db.file_map.read();
            Ok(format_document(&db, &params))
        }
    });

    // Rename
    let db_rename = db.clone();
    router.request::<Rename, _>(move |_, params: RenameParams| {
        let db = db_rename.clone();
        async move {
            let guard = db.file_map.read();
            Ok(rename_symbol(&db, &guard, &params))
        }
    });

    // FoldingRange - using FoldingRangeRequest
    let db_fold = db.clone();
    router.request::<FoldingRangeRequest, _>(move |_, params: FoldingRangeParams| {
        let db = db_fold.clone();
        async move { Ok(provide_folding_ranges(&db, &params)) }
    });

    // Code Action
    let db_action = db.clone();
    router.request::<CodeActionRequest, _>(move |_, params: CodeActionParams| {
        let db = db_action.clone();
        async move {
            let guard = db.file_map.read();
            Ok(provide_code_actions(&db, &guard, &params))
        }
    });

    // Document Symbols
    let db_doc = db.clone();
    router.request::<DocumentSymbolRequest, _>(move |_, params: DocumentSymbolParams| {
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
    router.notification::<DidOpenTextDocument>(
        move |_, params: DidOpenTextDocumentParams| {
            let path = path_from_uri(&params.text_document.uri);
            let content = params.text_document.text;
            let version = params.text_document.version;
            let _ = db_open.file_map.write().get_or_create(&path);
            let _ = tx_open.try_send(AnalysisMessage::FileChanged {
                path,
                content,
                version,
            });
            ControlFlow::Continue(())
        },
    );

    let db_change = db.clone();
    let tx_change = _analysis_tx.clone();
    router.notification::<DidChangeTextDocument>(
        move |_, params: DidChangeTextDocumentParams| {
            // FULL sync: the last content change carries the whole document.
            let path = path_from_uri(&params.text_document.uri);
            let version = params.text_document.version;
            if let Some(change) = params.content_changes.into_iter().last() {
                let _ = db_change.file_map.write().get_or_create(&path);
                let _ = tx_change.try_send(AnalysisMessage::FileChanged {
                    path,
                    content: change.text,
                    version,
                });
            }
            ControlFlow::Continue(())
        },
    );

    let db_close = db.clone();
    let tx_close = _analysis_tx.clone();
    router.notification::<DidCloseTextDocument>(
        move |_, params: DidCloseTextDocumentParams| {
            let path = path_from_uri(&params.text_document.uri);
            let _ = db_close.file_map.write().remove(&path);
            let _ = tx_close.try_send(AnalysisMessage::FileClosed { path });
            ControlFlow::Continue(())
        },
    );

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
