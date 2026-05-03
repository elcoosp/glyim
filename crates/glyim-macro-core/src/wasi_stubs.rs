use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

/// A deterministic WASI view.
///
/// For now wraps a default WasiCtx; deterministic clock/random overrides
/// will be implemented in a later iteration.
pub struct DeterministicWasi {
    ctx: WasiCtx,
    table: wasmtime::component::ResourceTable,
}

impl DeterministicWasi {
    pub fn new() -> Self {
        let ctx = WasiCtxBuilder::new().build();
        Self { ctx, table: wasmtime::component::ResourceTable::new() }
    }
}

impl WasiView for DeterministicWasi {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}
