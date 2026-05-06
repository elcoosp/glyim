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
    analysis_tx: mpsc::UnboundedSender<AnalysisMessage>,
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
            if let Some(path) = params.text_document.uri.to_file_path().ok() {
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
            if let Some(path) = params.text_document.uri.to_file_path().ok() {
                if let Some(change) = params.content_changes.into_iter().last() {
                    let _ = tx.send(AnalysisMessage::FileChanged {
                        path: path.clone(),
                        content: change.text,
                        version: params.text_document.version,
                    });
                    publish_diagnostics(&db, &path, &client);
                }
            }
            ControlFlow::Continue(())
        });
    }

    // didClose
    {
        let tx = analysis_tx.clone();
        router.notification::<notification::DidCloseTextDocument>(move |(), params| {
            if let Some(path) = params.text_document.uri.to_file_path().ok() {
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
    router.request::<request::Completion, _>(|(), _params| async {
        Ok::<Option<CompletionResponse>, _>(None)
    });

    // hover stub
    router.request::<request::HoverRequest, _>(|(), _params| async {
        Ok::<Option<Hover>, _>(None)
    });

    // goto definition stub
    router.request::<request::GotoDefinition, _>(|(), _params| async {
        Ok::<Option<GotoDefinitionResponse>, _>(None)
    });

    // references stub
    router.request::<request::References, _>(|(), _params| async {
        Ok::<Option<Vec<Location>>, _>(None)
    });

    // document symbol stub
    router.request::<request::DocumentSymbolRequest, _>(|(), _params| async {
        Ok::<Option<DocumentSymbolResponse>, _>(None)
    });

    router
}

fn publish_diagnostics(db: &AnalysisDatabase, path: &std::path::Path, client: &ClientSocket) {
    let file_id = { db.file_map.read().unwrap().get_by_path(path) };
    let Some(id) = file_id else { return };
    let diags: Vec<Diagnostic> = {
        db.diagnostics.read().unwrap().get(&id).cloned().unwrap_or_default()
    };
    if let Ok(uri) = Url::from_file_path(path) {
        let _ = client.notify::<notification::PublishDiagnostics>(
            PublishDiagnosticsParams { uri, diagnostics: diags, version: None }
        );
    }
}
