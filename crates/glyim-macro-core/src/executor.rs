use anyhow::{Result, anyhow};
use std::sync::Arc;
use wasmtime::WasmBacktraceDetails;
use wasmtime::*;

use glyim_macro_vfs::{ContentHash, ContentStore};

use crate::cache::{MacroExpansionCache, compute_cache_key};

/// The deterministic macro execution engine.
///
/// If a [`MacroExpansionCache`] is provided, the executor will:
/// 1. Compute a cache key before execution.
/// 2. Check the cache for a previous result.
/// 3. On cache hit, return the cached output without running Wasm.
/// 4. On cache miss, execute Wasm, store the result, and return it.
pub struct MacroExecutor {
    engine: Engine,
    cache: Option<MacroExpansionCache>,
}

impl MacroExecutor {
    /// Create a new executor without caching.
    pub fn new() -> Self {
        let mut config = Config::default();
        config.wasm_backtrace_max_frames(std::num::NonZero::new(64));
        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        let engine = Engine::new(&config).expect("failed to create wasmtime engine");
        Self {
            engine,
            cache: None,
        }
    }

    /// Create a new executor with a caching layer.
    pub fn new_with_cache(store: Arc<dyn ContentStore>) -> Self {
        let mut config = Config::default();
        config.wasm_backtrace_max_frames(std::num::NonZero::new(64));
        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        let engine = Engine::new(&config).expect("failed to create wasmtime engine");
        let cache = MacroExpansionCache::new(store);
        Self {
            engine,
            cache: Some(cache),
        }
    }

    /// Execute a macro Wasm module with the given input AST bytes.
    ///
    /// Returns the output bytes produced by the macro's `expand` export.
    pub fn execute(&self, wasm: &[u8], input: &[u8]) -> Result<Vec<u8>> {
        // Compute cache key and check cache
        let wasm_hash = ContentHash::of(wasm);
        let input_hash = ContentHash::of(input);

        if let Some(ref cache) = self.cache {
            let key = compute_cache_key(
                env!("CARGO_PKG_VERSION"),
                std::env::consts::ARCH,
                &wasm_hash,
                &input_hash,
                &[],
            );
            eprintln!("[executor] before lookup - key hex: {}", hex::encode(key));
            if let Some(data) = cache.lookup(&key) {
                eprintln!("[executor] cache HIT - returning {} bytes", data.len());
                return Ok(data);
            }
            eprintln!("[executor] cache MISS - will execute Wasm");
        }

        // ── Wasm execution ──────────────────────────────────────
        let module = Module::from_binary(&self.engine, wasm)?;
        let mut store = Store::new(&self.engine, ());
        let instance = Instance::new(&mut store, &module, &[])?;

        let maybe_memory = instance.get_memory(&mut store, "memory");

        let expand_fn = instance
            .get_func(&mut store, "expand")
            .ok_or_else(|| anyhow!("macro module must export a function named 'expand'"))?;

        if let Some(memory) = maybe_memory {
            let required_pages = ((input.len() * 2) as u64).div_ceil(65536) + 1;
            let current_pages = memory.size(&store);
            if current_pages < required_pages {
                let pages_to_grow = required_pages - current_pages;
                memory
                    .grow(&mut store, pages_to_grow)
                    .map_err(|e| anyhow!("failed to grow memory: {:?}", e))?;
            }
            memory
                .write(&mut store, 0, input)
                .map_err(|e| anyhow!("write input to memory: {e}"))?;
        }

        let output_offset = input.len() as i32;
        if let Some(ref mem) = maybe_memory
            && mem.size(&store) * 65536 < (input.len() as u64 * 3)
        {
            mem.grow(&mut store, 1)?;
        }

        let mut result = [Val::I32(0)];
        expand_fn
            .call(
                &mut store,
                &[
                    Val::I32(0),
                    Val::I32(input.len() as i32),
                    Val::I32(output_offset),
                ],
                &mut result,
            )
            .map_err(|e| {
                if let Some(bt) = e.downcast_ref::<wasmtime::WasmBacktrace>() {
                    eprintln!("Wasm backtrace:");
                    for (i, frame) in bt.frames().iter().enumerate() {
                        eprintln!(
                            "  frame {}: func_index={}, func_name={:?}, module_offset={:?}",
                            i,
                            frame.func_index(),
                            frame.func_name(),
                            frame.module_offset()
                        );
                    }
                }
                if let Some(trap) = e.downcast_ref::<wasmtime::Trap>() {
                    eprintln!("Trap cause: {:?}", trap);
                }
                anyhow!("macro expand call: {e}")
            })?;

        let output_len = match result[0] {
            Val::I32(len) => len as usize,
            _ => return Err(anyhow!("expand must return i32 output length")),
        };

        if let Some(ref _mem) = maybe_memory
            && output_len > input.len() * 2
        {
            return Err(anyhow!("macro output too large"));
        }

        let out = if let Some(ref mem) = maybe_memory {
            let mut buf = vec![0u8; output_len];
            mem.read(&store, output_offset as usize, &mut buf)
                .map_err(|e| anyhow!("read output from memory: {e}"))?;
            buf
        } else {
            vec![]
        };

        // Store result in cache if available
        if let Some(ref cache) = self.cache {
            let key = compute_cache_key(
                env!("CARGO_PKG_VERSION"),
                std::env::consts::ARCH,
                &wasm_hash,
                &input_hash,
                &[],
            );
            eprintln!(
                "[executor] storing result - key hex: {}, output len: {}",
                hex::encode(key),
                out.len()
            );
            if let Err(e) = cache.store(&key, &out) {
                eprintln!("[executor] cache store ERROR: {e}");
            }
        }

        Ok(out)
    }
}

impl Default for MacroExecutor {
    fn default() -> Self {
        Self::new()
    }
}
