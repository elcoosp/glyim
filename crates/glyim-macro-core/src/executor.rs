use anyhow::{Result, anyhow};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use wasmtime::*;

use glyim_macro_vfs::{ContentHash, ContentStore};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::p1::WasiP1Ctx;

use crate::cache::{MacroExpansionCache, compute_cache_key};

/// Fuel budget for macro execution — 200_000 instructions is generous
/// but prevents infinite loops. Based on Wasmtime fuel metering where each
/// "unit" corresponds roughly to one wasm instruction/basic-block.
const MACRO_FUEL_BUDGET: u64 = 200_000;

/// Simple bump-allocation state for the macro's linear memory.
#[derive(Default)]
struct AllocState {
    next_offset: u32,
}

/// Host environment visible to the macro (WASI + allocator).
struct MacroExecutionEnv {
    wasi: WasiP1Ctx,
    alloc_state: RefCell<AllocState>,
    table: wasmtime::component::ResourceTable,
}

/// The deterministic macro execution engine.
pub struct MacroExecutor {
    engine: Engine,
    cache: Option<MacroExpansionCache>,
    module_cache: RefCell<HashMap<ContentHash, Module>>,
}

impl MacroExecutor {
    /// Create a new executor without caching.
    pub fn new() -> Self {
        let mut config = Config::default();
        config.wasm_backtrace_max_frames(std::num::NonZero::new(64));
        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        config.consume_fuel(true);
        let engine = Engine::new(&config).expect("failed to create wasmtime engine");
        Self {
            engine,
            cache: None,
            module_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Create a new executor with a caching layer.
    pub fn new_with_cache(store: Arc<dyn ContentStore>) -> Self {
        let mut config = Config::default();
        config.wasm_backtrace_max_frames(std::num::NonZero::new(64));
        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        config.consume_fuel(true);
        let engine = Engine::new(&config).expect("failed to create wasmtime engine");
        let cache = MacroExpansionCache::new(store);
        Self {
            engine,
            cache: Some(cache),
            module_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Execute a macro Wasm module with the given input AST bytes.
    pub fn execute(&self, wasm: &[u8], input: &[u8]) -> Result<Vec<u8>> {
        let wasm_hash = ContentHash::of(wasm);
        let input_hash = ContentHash::of(input);

        // Compute cache key once – include OS for cross-platform safety
        let cache_key = if self.cache.is_some() {
            Some(compute_cache_key(
                env!("CARGO_PKG_VERSION"),
                &format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                &wasm_hash,
                &input_hash,
                &[],
            ))
        } else {
            None
        };

        // Check cache first
        if let Some(ref cache) = self.cache {
            if let Some(key) = cache_key {
                if let Some(data) = cache.lookup(&key) {
                    return Ok(data);
                }
            }
        }

        // Cache the compiled module
        let module = {
            let mut module_cache = self.module_cache.borrow_mut();
            if let Some(m) = module_cache.get(&wasm_hash) {
                m.clone()
            } else {
                let m = Module::from_binary(&self.engine, wasm)?;
                module_cache.insert(wasm_hash, m.clone());
                m
            }
        };

        // Build WASI context
        let wasi = WasiCtxBuilder::new()
            .inherit_stdio()
            .build_p1();
        let env = MacroExecutionEnv {
            wasi,
            alloc_state: RefCell::new(AllocState::default()),
            table: wasmtime::component::ResourceTable::new(),
        };
        let mut store = Store::new(&self.engine, env);
        store.set_fuel(MACRO_FUEL_BUDGET)
            .map_err(|e| anyhow!("set_fuel: {e}"))?;

        let mut linker = Linker::new(&self.engine);

        // Host function: allocate(size: i32) -> i32
        linker.func_wrap("env", "allocate", {
            |mut caller: Caller<'_, MacroExecutionEnv>, size: i32| -> i32 {
                let memory = match caller.get_export("memory") {
                    Some(e) => e.into_memory().unwrap(),
                    None => return 0,
                };
                let (ptr, needed) = {
                    let alloc_state = caller.data().alloc_state.borrow();
                    let ptr = alloc_state.next_offset;
                    let needed = u64::from(ptr) + size as u64;
                    (ptr, needed)
                };
                let current = memory.size(&caller) * 65536;
                if needed > current {
                    let pages = (needed - current).div_ceil(65536);
                    let _ = memory.grow(&mut caller, pages);
                }
                ptr as i32
            }
        })?;

        // Add WASI to the linker
        wasmtime_wasi::p1::wasi_snapshot_preview1::add_to_linker(&mut linker, |env: &mut MacroExecutionEnv| &mut env.wasi)?;

        let instance = linker.instantiate(&mut store, &module)?;
        let maybe_memory = instance.get_memory(&mut store, "memory");

        let expand_fn = instance
            .get_func(&mut store, "expand")
            .ok_or_else(|| anyhow!("macro module must export a function named 'expand'"))?;

        // Ensure memory has space for input + output
        if let Some(memory) = maybe_memory {
            let required_pages = ((input.len() * 2) as u64).div_ceil(65536) + 1;
            let current_pages = memory.size(&store);
            if current_pages < required_pages {
                memory
                    .grow(&mut store, required_pages - current_pages)
                    .map_err(|e| anyhow!("failed to grow memory: {:?}", e))?;
            }
            memory.write(&mut store, 0, input)
                .map_err(|e| anyhow!("write input to memory: {e}"))?;
        }

        let output_offset = input.len() as i32;

        let mut result = [Val::I32(0)];
        expand_fn.call(
            &mut store,
            &[
                Val::I32(0),
                Val::I32(input.len() as i32),
                Val::I32(output_offset),
            ],
            &mut result,
        ).map_err(|e| {
            if let Some(trap) = e.downcast_ref::<wasmtime::Trap>() {
                if *trap == wasmtime::Trap::OutOfFuel {
                    return anyhow!(
                        "macro execution exceeded fuel budget of {} instructions (infinite loop?)",
                        MACRO_FUEL_BUDGET
                    );
                }
            }
            anyhow!("macro expand call: {e}")
        })?;

        let output_len = match result[0] {
            Val::I32(len) => len as usize,
            _ => return Err(anyhow!("expand must return i32 output length")),
        };

        let out = if let Some(ref mem) = maybe_memory {
            let mut buf = vec![0u8; output_len];
            mem.read(&store, output_offset as usize, &mut buf)
                .map_err(|e| anyhow!("read output from memory: {e}"))?;
            buf
        } else {
            vec![]
        };

        // Store in cache
        if let Some(ref cache) = self.cache {
            if let Some(key) = cache_key {
                if let Err(e) = cache.store(&key, &out) {
                    eprintln!("[executor] cache store ERROR: {e}");
                }
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
