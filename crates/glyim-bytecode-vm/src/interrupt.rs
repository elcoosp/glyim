//! Host-interrupt gate: the external counterpart of the execution budget.
//!
//! The fuel gate ([`crate::Vm::with_fuel`]) is the machine's own stop
//! rail: a budgeted run refuses a dispatch when the budget is spent and
//! stops with the explicit [`crate::VmError::FuelExhausted`]. But the
//! refusal reason is the budget's story — an embedding host that must
//! stop a machine for its *own* reasons (a wall-clock deadline elapsed,
//! the user cancelled, a supervising policy revoked the run's
//! permission) had no channel: the only per-dispatch stops were the
//! internal budget and a fault. This module is that channel: the host
//! arms a [`Interrupt`] veto ([`crate::Vm::with_interrupt`]) and the
//! machine consults it once per pending dispatch, just after the fuel
//! gate and before the dispatch executes — vetoing stops the run with
//! the explicit [`crate::VmError::Interrupted`], carrying the host's
//! own reason string. A vetoed dispatch never executes and never
//! reaches the trace or fuel hooks (the same refusal contract as the
//! fuel gate), and the veto may never *start* a run that would
//! otherwise have stopped — it only stops earlier.

use std::any::Any;
use std::time::{Duration, Instant};

/// The pending dispatch a [`Interrupt`] is consulted on, handed to the
/// veto once per dispatch the machine is about to execute — the same
/// site the fault-site tracking ([`crate::Vm::fault_func`]) has
/// refreshed, so a veto names exactly the dispatch it refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterruptProbe {
    /// The function index containing the pending dispatch.
    pub func: usize,
    /// The byte offset of the pending dispatch (the dispatch site).
    pub pc: usize,
    /// The dispatch's 1-based lifetime number — the count the *pending*
    /// dispatch would receive ([`crate::Vm::executed`] + 1: the veto
    /// fires before the dispatch runs, so the number is the one it
    /// would have gotten, not one it got).
    pub dispatch: u64,
}

/// The host-side veto the machine consults once per pending dispatch.
///
/// The contract mirrors the observation seams' sink contract, with the
/// one deliberate difference that defines the gate: the veto may *stop*
/// the run (returning `Some(reason)` refuses the pending dispatch) but
/// may never otherwise change it — no way to mutate machine state, to
/// redirect execution, or to veto retroactively. Implementors must
/// treat [`Interrupt::interrupted`] as a pure question; a panicking
/// veto aborts the run, which is the embedding host's explicit choice.
/// The [`Interrupt::as_any`] hook lets a caller recover the typed veto
/// after [`crate::Vm::take_interrupt`].
pub trait Interrupt: 'static {
    /// Consult the veto on one pending dispatch: `Some(reason)` refuses
    /// the dispatch (the run stops with
    /// [`crate::VmError::Interrupted`], `reason` travels in the error),
    /// `None` lets it execute. Fired once per pending dispatch, just
    /// after the fuel gate — a fuel-refused dispatch is never consulted
    /// (the budget's refusal is the budget story, even when a veto is
    /// armed).
    fn interrupted(&mut self, probe: &InterruptProbe) -> Option<&'static str>;

    /// Downcast hook: hand the veto back to its typed owner after
    /// `Vm::take_interrupt`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A veto armed with a wall-clock deadline: the run is refused once the
/// deadline has passed (reason `"deadline"`).
///
/// The deadline is captured at construction —
/// [`Deadline::from_now`] freezes it at arming time, so a machine
/// built with `from_now(30s)` is vetoed by a run that starts 31
/// seconds later even if that run is fast (the deadline is a
/// point in time, not a per-run timer). For a fresh budget per run,
/// build a fresh `Vm` (or re-arm a fresh `Deadline`) per run.
#[derive(Debug, Clone, Copy)]
pub struct Deadline {
    /// The instant the veto fires from.
    deadline: Instant,
}

impl Deadline {
    /// Arm the veto to fire from `deadline` on (the instant is
    /// compared against the wall clock at every consulted dispatch).
    #[must_use]
    pub fn after(deadline: Instant) -> Deadline {
        Deadline { deadline }
    }

    /// Arm the veto to fire `timeout` from the current instant (the
    /// deadline is frozen at arming time, not per dispatch).
    #[must_use]
    pub fn from_now(timeout: Duration) -> Deadline {
        Deadline {
            deadline: Instant::now() + timeout,
        }
    }
}

impl Interrupt for Deadline {
    fn interrupted(&mut self, _probe: &InterruptProbe) -> Option<&'static str> {
        if Instant::now() >= self.deadline {
            Some("deadline")
        } else {
            None
        }
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// A veto over a host closure: every consultation calls the closure
/// with the pending dispatch's [`InterruptProbe`], and its answer is
/// the veto's answer — the adapter that lets an embedding host veto
/// from its own state (a deadline channel, an atomic flag, a supervisor
/// message queue) without implementing [`Interrupt`] by hand.
#[derive(Debug)]
pub struct InterruptFn<F> {
    /// The host's closure (called once per consulted dispatch).
    f: F,
}

impl<F> InterruptFn<F>
where
    F: FnMut(&InterruptProbe) -> Option<&'static str> + 'static,
{
    /// Wrap a closure into a veto. The closure is called once per
    /// consulted dispatch; its `Some(reason)` is the veto's answer.
    #[must_use]
    pub fn new(f: F) -> InterruptFn<F> {
        InterruptFn { f }
    }
}

impl<F> Interrupt for InterruptFn<F>
where
    F: FnMut(&InterruptProbe) -> Option<&'static str> + 'static,
{
    fn interrupted(&mut self, probe: &InterruptProbe) -> Option<&'static str> {
        (self.f)(probe)
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod interrupt_tests {
    use crate::interrupt::{Deadline, Interrupt, InterruptFn, InterruptProbe};
    use crate::trace::TraceCollector;
    use crate::{Function, Module, Opcode, Value, Vm, VmError, decode_module, encode_module};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    fn i64le(v: i64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// helper(n) = n * 2 + main returning helper(21): the trace/fuel
    /// golden module's exact shape — 9 dispatches, 4 in the callee, 5 in
    /// the caller. Dispatch 4 is the callee's first dispatch (fn 0 pc 0).
    fn golden_module() -> Module {
        let mut helper = Vec::new();
        helper.push(op(Opcode::LoadConst));
        helper.extend_from_slice(&i64le(2));
        helper.push(op(Opcode::LoadLocal));
        helper.extend_from_slice(&u32le(1)); // MIR ABI: arg at local 1
        helper.push(op(Opcode::Mul));
        helper.push(op(Opcode::Return));
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0)); // fn index 0: helper
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(21));
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(0)); // dest local
        main.extend_from_slice(&u32le(31)); // resume byte offset
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        Module::with_strings(
            vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
            vec!["abc".to_string()],
            1,
        )
    }

    /// A countdown loop over local 0 — the never-terminating shape: an
    /// unbounded run of it only stops by refusal, so a veto is what
    /// brings it home.
    fn countdown_module(seed: i64) -> Module {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&seed.to_le_bytes());
        code.push(op(Opcode::StoreLocal));
        code.extend_from_slice(&u32le(0));
        let head = code.len();
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::Sub));
        code.push(op(Opcode::StoreLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(0));
        code.push(op(Opcode::Gt));
        code.push(op(Opcode::JumpIf));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::Return));
        Module::new(vec![Function::with_blocks(code, 1, 1, vec![head])], 0)
    }

    /// A veto from the host closure `f`, which sees every probe.
    fn veto_with<F>(f: F) -> Box<dyn Interrupt>
    where
        F: FnMut(&InterruptProbe) -> Option<&'static str> + 'static,
    {
        Box::new(InterruptFn::new(f))
    }

    /// A host veto stops the run with the explicit error: the golden
    /// module's fourth dispatch (the callee's first) is refused, the
    /// error carries the three completed dispatches and the host's own
    /// reason, and the reason survives into the error's text.
    #[test]
    fn vetoed_run_reports_executed_and_reason() {
        let module = golden_module();
        let mut vm = Vm::new().with_interrupt(veto_with(|probe| {
            if probe.dispatch >= 4 {
                Some("host policy")
            } else {
                None
            }
        }));
        let err = vm.run_module(&module).expect_err("the veto stops the run");
        assert_eq!(
            err,
            VmError::Interrupted {
                executed: 3,
                reason: "host policy",
            },
            "three dispatches completed before the fourth was refused"
        );
        assert!(
            err.to_string().contains("host policy"),
            "the reason travels into the error's text: {err}"
        );
    }

    /// A vetoed dispatch never executes and never reaches the trace or
    /// fuel hooks — the same refusal contract as the fuel gate: the
    /// countdown's fourth dispatch is refused, the trace holds exactly
    /// the three executed steps, and the fuel seam (armed, unbounded)
    /// holds exactly three ticks (the refused dispatch never ticks).
    #[test]
    fn vetoed_dispatch_never_executes_or_ticks() {
        let module = countdown_module(10);
        let trace = TraceCollector::new();
        let fuel = crate::fuel::FuelCollector::new();
        let mut vm = Vm::new()
            .with_fuel_sink(Box::new(fuel))
            .with_trace(Box::new(trace))
            .with_interrupt(veto_with(|probe| {
                if probe.dispatch >= 4 {
                    Some("limit")
                } else {
                    None
                }
            }));
        let err = vm.run_module(&module).expect_err("the veto stops the loop");
        assert_eq!(
            err,
            VmError::Interrupted {
                executed: 3,
                reason: "limit",
            }
        );
        let mut traced = vm.take_trace().expect("trace attached");
        assert_eq!(
            traced
                .as_any()
                .downcast_ref::<TraceCollector>()
                .expect("trace collector")
                .len(),
            3,
            "one step per executed dispatch, none for the refused one"
        );
        let mut boxed = vm.take_fuel_sink().expect("fuel attached");
        assert_eq!(
            boxed
                .as_any()
                .downcast_ref::<crate::fuel::FuelCollector>()
                .expect("fuel collector")
                .len(),
            3,
            "one tick per executed dispatch, none for the refused one"
        );
    }

    /// The probe names the pending dispatch exactly: the golden module's
    /// fourth dispatch is the callee's first (fn 0 pc 0), and the veto
    /// sees `dispatch` = the number it would receive (completed + 1).
    #[test]
    fn probe_names_the_pending_dispatch() {
        let module = golden_module();
        let probes = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&probes);
        let mut vm = Vm::new().with_interrupt(veto_with(move |probe| {
            seen.borrow_mut().push(*probe);
            if probe.dispatch >= 4 {
                Some("limit")
            } else {
                None
            }
        }));
        let err = vm.run_module(&module).expect_err("vetoed at dispatch 4");
        assert_eq!(
            err,
            VmError::Interrupted {
                executed: 3,
                reason: "limit",
            }
        );
        let probes = probes.borrow().clone();
        assert_eq!(probes.len(), 4, "one consultation per pending dispatch");
        assert_eq!(
            probes
                .iter()
                .map(|p| (p.func, p.pc, p.dispatch))
                .collect::<Vec<_>>(),
            vec![(1, 0, 1), (1, 9, 2), (1, 18, 3), (0, 0, 4)],
            "main's three dispatches, then the callee's first — refused"
        );
    }

    /// A VM without a veto behaves exactly as before: the run completes,
    /// and no veto comes back from `take_interrupt`.
    #[test]
    fn no_interrupt_by_default() {
        let module = golden_module();
        let mut vm = Vm::new();
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        assert!(vm.take_interrupt().is_none());
    }

    /// The veto comes back typed through `take_interrupt` + `as_any`:
    /// the host recovers its own state (here, how many consultations a
    /// never-vetoing deadline saw).
    #[test]
    fn take_interrupt_returns_the_box_typed() {
        let module = golden_module();
        let mut vm = Vm::new().with_interrupt(Box::new(Deadline::after(
            Instant::now() + Duration::from_secs(3600),
        )));
        let value = vm.run_module(&module).expect("runs inside the deadline");
        assert_eq!(value, Value::Int(42));
        let mut boxed = vm.take_interrupt().expect("veto still attached");
        let deadline = boxed
            .as_any()
            .downcast_ref::<Deadline>()
            .expect("the deadline came back through as_any");
        let _ = *deadline;
        assert!(vm.take_interrupt().is_none(), "take detaches");
    }

    /// The fuel gate outranks the veto: a budget spent mid-run stops
    /// with `FuelExhausted` even when a veto is armed (the budget's
    /// refusal is the budget story, unchanged for existing users), while
    /// a roomy budget under an armed veto stops with `Interrupted`.
    #[test]
    fn fuel_outranks_the_veto() {
        let module = countdown_module(10);
        // The veto arms at dispatch 3 — the exact dispatch a budget of 2
        // refuses — so if the veto outranked the gate, this would be an
        // Interrupted; the fuel gate fires first and the veto is never
        // consulted on the refused dispatch.
        let veto = veto_with(|probe| {
            if probe.dispatch >= 3 {
                Some("host policy")
            } else {
                None
            }
        });
        let mut vm = Vm::new().with_fuel(2).with_interrupt(veto);
        let err = vm.run_module(&module).expect_err("the budget wins");
        assert_eq!(
            err,
            VmError::FuelExhausted { executed: 2 },
            "fuel gate first: the budget's refusal is the budget story"
        );
        let mut vm = Vm::new().with_fuel(1000).with_interrupt(veto_with(|probe| {
            if probe.dispatch >= 5 {
                Some("host policy")
            } else {
                None
            }
        }));
        let err = vm.run_module(&module).expect_err("the veto wins");
        assert_eq!(
            err,
            VmError::Interrupted {
                executed: 4,
                reason: "host policy",
            }
        );
    }

    /// The `Deadline` impl: a deadline already passed vetoes before the
    /// first dispatch (zero completed dispatches, reason `"deadline"`),
    /// and a deadline far in the future lets the run complete.
    #[test]
    fn deadline_veto_arms_and_expires() {
        let module = golden_module();
        let mut vm = Vm::new().with_interrupt(Box::new(Deadline::after(Instant::now())));
        let err = vm.run_module(&module).expect_err("the deadline passed");
        assert_eq!(
            err,
            VmError::Interrupted {
                executed: 0,
                reason: "deadline",
            },
            "the first dispatch is refused — nothing completed"
        );
        let mut vm =
            Vm::new().with_interrupt(Box::new(Deadline::from_now(Duration::from_secs(3600))));
        let value = vm.run_module(&module).expect("runs inside the deadline");
        assert_eq!(value, Value::Int(42));
    }

    /// A never-vetoing veto is read-only: the result matches the
    /// unattached run exactly, and the veto is consulted every dispatch
    /// (here, counted by the host closure).
    #[test]
    fn never_vetoing_interrupt_is_read_only() {
        let module = golden_module();
        let unattached = Vm::new().run_module(&module).expect("unattached run");
        let consulted = Rc::new(RefCell::new(0u64));
        let seen = Rc::clone(&consulted);
        let mut vm = Vm::new().with_interrupt(veto_with(move |_| {
            *seen.borrow_mut() += 1;
            None
        }));
        let vetoed = vm.run_module(&module).expect("never-vetoing run");
        assert_eq!(vetoed, unattached);
        assert_eq!(
            *consulted.borrow(),
            9,
            "one consultation per pending dispatch"
        );
    }

    /// The veto composes with the module image: decode the shipped bytes,
    /// then veto the loaded module's run — the image round trip and the
    /// gate are independent machines.
    #[test]
    fn interrupt_composes_with_module_image() {
        let module = golden_module();
        let bytes = encode_module(&module);
        let loaded = decode_module(&bytes).expect("decode");
        let mut vm = Vm::new().with_interrupt(veto_with(|probe| {
            if probe.dispatch >= 2 {
                Some("host policy")
            } else {
                None
            }
        }));
        let err = vm.run_module(&loaded).expect_err("vetoed at dispatch 2");
        assert_eq!(
            err,
            VmError::Interrupted {
                executed: 1,
                reason: "host policy",
            }
        );
    }

    /// A veto fires at most once per run (the refusal ends the run), so
    /// a one-shot veto is expressible: the host flips its state at the
    /// site it cares about and the machine never consults it again.
    #[test]
    fn one_shot_veto_at_a_site() {
        let module = countdown_module(100);
        // Veto exactly once, at the loop head's first reload (pc 14 —
        // the loop head the preamble jumps to; the fuel golden pins the
        // third dispatch at fn 0 pc 14): the preamble's two dispatches
        // complete, the loop's first load is refused.
        let mut vm = Vm::new().with_interrupt(veto_with(|probe| {
            if probe.pc == 14 && probe.dispatch == 3 {
                Some("site policy")
            } else {
                None
            }
        }));
        let err = vm.run_module(&module).expect_err("vetoed at the loop head");
        assert_eq!(
            err,
            VmError::Interrupted {
                executed: 2,
                reason: "site policy",
            }
        );
    }
}
