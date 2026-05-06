pub fn cmd_lsp() -> i32 {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async { glyim_lsp::server::run_server(None).await; });
    0
}
