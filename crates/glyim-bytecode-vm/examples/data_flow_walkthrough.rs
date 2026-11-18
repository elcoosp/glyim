//! Consumer walkthrough of the data-flow seam: which reads consumed which
//! writes (`glyim_bytecode_vm::dataflow`).
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example data_flow_walkthrough
//! ```
//!
//! The example pairs a [`DataFlowCollector`] with both hooks of one `Vm`
//! ([`Vm::with_watch`] carries the write stream, [`Vm::with_reads`] the
//! read stream — the shared-state clone is what makes it one fold), runs a
//! small program, and prints the fold: which slots were written and read,
//! which dispatch last defined each slot, and — for a program that reads a
//! slot it never stored in — the displayed `uninitialized_reads` report.
//! The sibling `local_watch_walkthrough` walks the write stream alone;
//! this one answers the *pairing* question the write stream cannot.

use glyim_bytecode_vm::{
    DataFlowCollector, FlowEvent, FlowRecorder, Function, Module, Opcode, Value, Vm,
    render_data_flow,
};
use std::any::Any;

fn i64le(v: i64) -> [u8; 8] {
    v.to_le_bytes()
}
fn u32le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}
fn op(o: Opcode) -> u8 {
    o as u8
}

/// main: stores 7 into local 1, computes local 2 = local 1 * 2, returns
/// local 2 — a write-read-write-read chain over two slots.
fn pairing_program() -> Module {
    let mut code = Vec::new();
    code.push(op(Opcode::LoadConst));
    code.extend_from_slice(&i64le(7));
    code.push(op(Opcode::StoreLocal));
    code.extend_from_slice(&u32le(1)); // write local 1 = 7
    code.push(op(Opcode::LoadLocal));
    code.extend_from_slice(&u32le(1)); // read local 1 -> def pairs here
    code.push(op(Opcode::LoadConst));
    code.extend_from_slice(&i64le(2));
    code.push(op(Opcode::Mul));
    code.push(op(Opcode::StoreLocal));
    code.extend_from_slice(&u32le(2)); // write local 2 = 14
    code.push(op(Opcode::LoadLocal));
    code.extend_from_slice(&u32le(2)); // read local 2 -> def pairs here
    code.push(op(Opcode::Return));
    Module::new(vec![Function::new(code, 3, 0)], 0)
}

/// main: reads local 0 twice without ever storing it — the zero-init
/// consumption the seam displays instead of swallowing.
fn uninitialized_program() -> Module {
    let mut code = Vec::new();
    code.push(op(Opcode::LoadLocal));
    code.extend_from_slice(&u32le(0));
    code.push(op(Opcode::LoadLocal));
    code.extend_from_slice(&u32le(0));
    code.push(op(Opcode::Return));
    Module::new(vec![Function::new(code, 1, 0)], 0)
}

/// Run a module with one paired collector on both hooks; returns the fold
/// and the run's value (which must equal the unpaired run's — the seam is
/// read-only).
fn paired_run(module: &Module) -> (DataFlowCollector, Value) {
    let collector = DataFlowCollector::new();
    let mut vm = Vm::new()
        .with_watch(Box::new(collector.clone()))
        .with_reads(Box::new(collector));
    let value = vm.run_module(module).expect("module runs");
    let mut boxed = vm.take_reads().expect("read sink still attached");
    let sink: &mut dyn Any = boxed.as_any();
    let collector = sink
        .downcast_mut::<DataFlowCollector>()
        .expect("the collector came back")
        .clone();
    (collector, value)
}

fn main() {
    // 1. Pairing: every read resolves to the dispatch that last defined
    //    its slot; the fold is one shared state across the two hooks.
    let module = pairing_program();
    let (collector, value) = paired_run(&module);
    assert_eq!(value, Value::Int(14), "7 * 2");
    println!("paired run value: {value:?}");
    print!("{}", render_data_flow(&collector));

    // 2. The transcript: a FlowRecorder attached the same way captures the
    //    interleaved write/read order, replayable through from_transcript.
    let recorder = FlowRecorder::new();
    let mut vm = Vm::new()
        .with_watch(Box::new(recorder.clone()))
        .with_reads(Box::new(recorder));
    vm.run_module(&module).expect("recorded run");
    let mut boxed = vm.take_reads().expect("recorder attached");
    let transcript = boxed
        .as_any()
        .downcast_ref::<FlowRecorder>()
        .expect("the recorder came back")
        .transcript();
    let replay = DataFlowCollector::from_transcript(transcript.iter().cloned());
    assert_eq!(replay, collector, "the re-fold reproduces the fold");
    for event in &transcript {
        match event {
            FlowEvent::Write(w) => println!(
                "  write fn {} slot {} dispatch {}",
                w.func, w.local, w.dispatch
            ),
            FlowEvent::Read(r) => println!(
                "  read  fn {} slot {} dispatch {}",
                r.func, r.local, r.dispatch
            ),
        }
    }

    // 3. Uninitialized reads: a slot read before any write is displayed,
    //    never silently consumed from the frame's zero-init.
    let module = uninitialized_program();
    let (collector, value) = paired_run(&module);
    assert_eq!(value, Value::Int(0), "the zero-init value");
    println!("uninitialized-read run value: {value:?}");
    print!("{}", render_data_flow(&collector));
    assert_eq!(collector.uninitialized_read_total(), 2);
}
