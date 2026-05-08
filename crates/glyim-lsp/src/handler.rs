#![allow(clippy::let_underscore_future)]

use crate::AnalysisDatabase;
use crate::driver::AnalysisMessage;
use std::sync::Arc;
use std::ops::ControlFlow;
use tokio::sync::mpsc;
use async_lsp::router::Router;

use async_lsp::ClientSocket;
use async_lsp::lsp_types::*;

pub fn build_router(
    db: Arc<AnalysisDatabase>,
    analysis_tx: mpsc::Sender<AnalysisMessage>,
    client: ClientSocket,
) -> Router<()> {
    let mut router = Router::new(());

    // Initialize
    router.request::<request::Initialize, _>(|(), _params| async {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
                completion_provider: Some(CompletionOptions::default()),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    });

    // Initialized
    {
        let client = client.clone();
        router.notification::<notification::Initialized>(move |(), _params| {
            let _ = client.notify::<notification::LogMessage>(LogMessageParams {
                typ: MessageType::INFO,
                message: "Glyim language server started".into(),
            });
            ControlFlow::Continue(())
        });
    }

    // didOpen
    {
        let db = db.clone();
        let tx = analysis_tx.clone();
        let client = client.clone();
        router.notification::<notification::DidOpenTextDocument>(move |(), params| {
            if let Ok(path) = params.text_document.uri.to_file_path() {
                let _ = tx.send(AnalysisMessage::FileChanged {
                    path: path.clone(),
                    content: params.text_document.text,
                    version: params.text_document.version,
                });
                publish_diagnostics(&db, &path, &client);
            }
            ControlFlow::Continue(())
        });
    }

    // didChange
    {
        let db = db.clone();
        let tx = analysis_tx.clone();
        let client = client.clone();
        router.notification::<notification::DidChangeTextDocument>(move |(), params| {
            if let Ok(path) = params.text_document.uri.to_file_path()
                && let Some(change) = params.content_changes.into_iter().last() {
                    let _ = tx.send(AnalysisMessage::FileChanged {
                        path: path.clone(),
                        content: change.text,
                        version: params.text_document.version,
                    });
                    publish_diagnostics(&db, &path, &client);
                }
            ControlFlow::Continue(())
        });
    }

    // didClose
    {
        let tx = analysis_tx.clone();
        router.notification::<notification::DidCloseTextDocument>(move |(), params| {
            if let Ok(path) = params.text_document.uri.to_file_path() {
                let _ = tx.send(AnalysisMessage::FileClosed { path });
            }
            ControlFlow::Continue(())
        });
    }

    // shutdown
    {
        let tx = analysis_tx.clone();
        router.request::<request::Shutdown, _>(move |(), ()| {
            let _ = tx.send(AnalysisMessage::Shutdown);
            async { Ok(()) }
        });
    }

    // completion stub
    {
        let db = db.clone();
        router.request::<request::Completion, _>(move |(), params| {
            let db = db.clone();
            async move {
                let fm = db.file_map.read();
                Ok(crate::completion::provide_completions(&db, &fm, &params))
            }
        });
    }

    // hover stub
    {
        let db = db.clone();
        router.request::<request::HoverRequest, _>(move |(), params| {
            let db = db.clone();
            async move {
                let fm = db.file_map.read();
                Ok(crate::hover::provide_hover(&db, &fm, &params))
            }
        });
    }

    // goto definition stub
    {
        let db = db.clone();
        router.request::<request::GotoDefinition, _>(move |(), params| {
            let db = db.clone();
            async move {
                let fm = db.file_map.read();
                Ok(crate::navigation::goto_definition(&db, &fm, &params))
            }
        });
    }

    // references stub
    {
        let db = db.clone();
        router.request::<request::References, _>(move |(), params| {
            let db = db.clone();
            async move {
                let fm = db.file_map.read();
                Ok(crate::navigation::find_references(&db, &fm, &params))
            }
        });
    }

    // document symbol stub
    {
        let db = db.clone();
        router.request::<request::DocumentSymbolRequest, _>(move |(), params| {
            let db = db.clone();
            async move {
                let fm = db.file_map.read();
                Ok(crate::navigation::document_symbols(&db, &fm, &params))
            }
        });
    }

    router
}

fn publish_diagnostics(db: &AnalysisDatabase, path: &std::path::Path, client: &ClientSocket) {
    let file_id = { db.file_map.read().get_by_path(path) };
    let Some(id) = file_id else { return };
    let diags: Vec<Diagnostic> = {
        db.diagnostics.read().get(&id).cloned().unwrap_or_default()
    };
    if let Ok(uri) = Url::from_file_path(path) {
        let _ = client.notify::<notification::PublishDiagnostics>(
            PublishDiagnosticsParams { uri, diagnostics: diags, version: None }
        );
    }
}
