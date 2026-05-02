use anyhow::{anyhow, Result};
use wasmtime::WasmBacktraceDetails;
use wasmtime::*;

/// The deterministic macro execution engine.
pub struct MacroExecutor {
    engine: Engine,
}

impl MacroExecutor {
    /// Create a new executor with the default wasmtime configuration.
    pub fn new() -> Self {
        let mut config = Config::default();
        config.wasm_backtrace_max_frames(std::num::NonZero::new(64));
        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        // Fuel and epoch disabled for now (will re‑enable later with proper tuning)
        let engine = Engine::new(&config).expect("failed to create wasmtime engine");
        Self { engine }
    }

    /// Execute a macro Wasm module with the given input AST bytes.
    ///
    /// Returns the output bytes produced by the macro's `expand` export.
    pub fn execute(&self, wasm: &[u8], input: &[u8]) -> Result<Vec<u8>> {
        let module = Module::from_binary(&self.engine, wasm)?;
        let mut store = Store::new(&self.engine, ());
        let instance = Instance::new(&mut store, &module, &[])?;

        let maybe_memory = instance.get_memory(&mut store, "memory");

        let expand_fn = instance
            .get_func(&mut store, "expand")
            .ok_or_else(|| anyhow!("macro module must export a function named 'expand'"))?;

        if let Some(memory) = maybe_memory {
            // Prepare linear memory: grow if needed
            let required_pages = ((input.len() * 2) as u64 + 65536 - 1) / 65536 + 1;
            let current_pages = memory.size(&store);
            if current_pages < required_pages {
                let pages_to_grow = required_pages - current_pages;
                memory.grow(&mut store, pages_to_grow)
                    .map_err(|e| anyhow!("failed to grow memory: {:?}", e))?;
            }

            // Write input data at offset 0
            memory.write(&mut store, 0, input)
                .map_err(|e| anyhow!("write input to memory: {e}"))?;
        }

        // Write input data at offset 0
        // Reserve output buffer after input (offset = input.len())
        let output_offset = input.len() as i32;
        if let Some(ref mem) = maybe_memory {
            // Ensure enough memory for output buffer (max input size * 2)
            if mem.size(&store) * 65536 < (input.len() as u64 * 3) {
                mem.grow(&mut store, 1)?;
            }
        }

        // Call expand(input_ptr=0, input_len=input.len() as i32, output_ptr=output_offset)
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

        if let Some(ref _mem) = maybe_memory {
            if output_len > input.len() * 2 {
                return Err(anyhow!("macro output too large"));
            }
        }

        // Read output bytes (or return empty if no memory)
        let out = if let Some(ref mem) = maybe_memory {
            let mut buf = vec![0u8; output_len];
            mem.read(&store, output_offset as usize, &mut buf)
                .map_err(|e| anyhow!("read output from memory: {e}"))?;
            buf
        } else {
            vec![]
        };

        Ok(out)
    }
}
