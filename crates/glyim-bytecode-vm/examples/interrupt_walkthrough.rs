//! Host-interrupt walkthrough: arm a veto on a `Vm`, watch it stop a
//! never-terminating machine with the host's own reason, and see the
//! gate compose with the fuel budget it rides beside.
//!
//! The fuel gate (`Vm::with_fuel`) stops an infinite loop in a loaded
//! artifact with the explicit `VmError::FuelExhausted` — but the
//! refusal reason is the budget's story. An embedding host that must
//! stop a machine for its *own* reasons (a wall-clock deadline, a
//! user cancel, a supervising policy) arms a `Interrupt` veto through
//! `Vm::with_interrupt`: the machine consults it once per pending
//! dispatch, just after the fuel gate, and a veto stops the run with
//! the explicit `VmError::Interrupted`, carrying the host's own reason
//! string. A vetoed dispatch never executes and never reaches the
//! trace or fuel hooks, the fuel gate outranks the veto, and
//! `Vm::take_interrupt` hands the veto back after the run. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example interrupt_walkthrough
//! ```

use glyim_bytecode_vm::{
    Deadline, Function, InterruptFn, InterruptProbe, Module, Opcode, Vm, VmError, encode_module,
};
use std::time::{Duration, Instant};

/// helper(n): returns `n * 2`. No jumps, so it carries no block-offset
/// table (the VM's raw byte-offset target mode).
fn helper() -> Function {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&2i64.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // MIR ABI: arg at local 1
    code.push(Opcode::Mul as u8);
    code.push(Opcode::Return as u8);
    Function::new(code, 2, 1)
}

/// `main()`: returns `helper(21)` — 9 dispatches total (5 in main, 4 in
/// the helper), the two-function module the other VM walkthroughs use.
fn main_fn() -> Function {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: helper
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&21i64.to_le_bytes());
    code.push(Opcode::Call as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // argc
    code.extend_from_slice(&0u32.to_le_bytes()); // dest local
    code.extend_from_slice(&31u32.to_le_bytes()); // resume byte offset
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Return as u8);
    Function::new(code, 1, 0)
}

/// A countdown loop over local 0 — the never-terminating shape: an
/// unbounded run of it only stops by refusal, so a veto (or a budget)
/// is what brings it home.
fn countdown_module(seed: i64) -> Module {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&seed.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    let head = code.len();
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&1i64.to_le_bytes());
    code.push(Opcode::Sub as u8);
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&0i64.to_le_bytes());
    code.push(Opcode::Gt as u8);
    code.push(Opcode::JumpIf as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Return as u8);
    Module::new(vec![Function::with_blocks(code, 1, 1, vec![head])], 0)
}

fn main() {
    let module = Module::with_strings(vec![helper(), main_fn()], vec!["abc".to_string()], 1);
    let bytes = encode_module(&module);

    // A `Deadline` veto with a deadline already passed: the machine is
    // refused before its first dispatch — zero completed dispatches, the
    // host's `"deadline"` reason in the error.
    let mut vm = Vm::new().with_interrupt(Box::new(Deadline::after(Instant::now())));
    let err = vm.run_module(&module).expect_err("the deadline passed");
    println!("expired deadline: {err}");
    assert_eq!(
        err,
        VmError::Interrupted {
            executed: 0,
            reason: "deadline",
        }
    );
    // The veto comes back typed, ready to re-arm on the next machine.
    let mut veto = vm.take_interrupt().expect("the veto is still attached");
    let _ = veto.as_any().downcast_ref::<Deadline>().expect("deadline");
    println!("take_interrupt handed the Deadline back (typed via as_any)");
    println!();

    // A closure veto (`InterruptFn`) over the encoded artifact: refuse
    // everything from the fourth dispatch on — three dispatches complete
    // (main's two constants + the call), the callee's first dispatch is
    // the refused one, and the probe names it exactly.
    let loaded = glyim_bytecode_vm::decode_module(&bytes).expect("decode");
    let vetoed = Vm::new()
        .with_interrupt(Box::new(InterruptFn::new(|probe: &InterruptProbe| {
            if probe.dispatch >= 4 {
                Some("host policy")
            } else {
                None
            }
        })))
        .run_module(&loaded)
        .expect_err("the veto stops the run");
    println!("closure veto over the decoded image: {vetoed}");
    assert_eq!(
        vetoed,
        VmError::Interrupted {
            executed: 3,
            reason: "host policy",
        }
    );
    println!();

    // The fuel gate outranks the veto: the veto arms at dispatch 3 — the
    // exact dispatch a budget of 2 refuses — and the budget's refusal is
    // still the budget story (`FuelExhausted`), unchanged for existing
    // users. With a roomy budget the same veto wins instead.
    let countdown = countdown_module(10);
    let mut vm = Vm::new()
        .with_fuel(2)
        .with_interrupt(Box::new(InterruptFn::new(|probe: &InterruptProbe| {
            if probe.dispatch >= 3 {
                Some("host policy")
            } else {
                None
            }
        })));
    let err = vm.run_module(&countdown).expect_err("the budget wins");
    println!("budget 2 + veto armed at 3: {err} (the fuel gate fired first)");
    assert_eq!(err, VmError::FuelExhausted { executed: 2 });
    let mut vm = Vm::new()
        .with_fuel(1000)
        .with_interrupt(Box::new(InterruptFn::new(|probe: &InterruptProbe| {
            if probe.dispatch >= 5 {
                Some("host policy")
            } else {
                None
            }
        })));
    let err = vm.run_module(&countdown).expect_err("the veto wins");
    println!("budget 1000 + veto armed at 5: {err}");
    assert_eq!(
        err,
        VmError::Interrupted {
            executed: 4,
            reason: "host policy",
        }
    );
    println!();

    // A silent veto is invisible: the run completes with the same value
    // the unattached machine produces — observation and control never
    // change execution until they refuse.
    let mut vm = Vm::new().with_interrupt(Box::new(Deadline::from_now(Duration::from_secs(3600))));
    let value = vm.run_module(&module).expect("runs inside the deadline");
    println!("far-future deadline over the golden module: {value:?}");
    assert_eq!(value, glyim_bytecode_vm::Value::Int(42));
}
