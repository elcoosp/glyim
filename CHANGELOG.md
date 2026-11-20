# Changelog

All notable changes to Glyim are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Fuel-observation seam (`glyim_bytecode_vm::fuel`)**: the *budget
  counterpart* of the trace seam — the fuel gate (`Vm::with_fuel`) is
  the machine's execution-budget rail (an infinite loop in a loaded
  artifact stops with the explicit `VmError::FuelExhausted` instead of
  hanging an embedding host), but the budget itself was observable
  only through its refusal: nothing answered *where* the budget went —
  how much burned per function, at which site the last allowed
  dispatch ran, what remained at each step. The VM now hands one
  `FuelTick` (the executing function, the dispatch site, the
  dispatch's 1-based lifetime number, and the budget remaining after
  this dispatch's spend — `None` for an unbounded run, still
  observable as burn) to a `FuelSink` per executed dispatch, just
  after the budget for that dispatch has been spent and before the
  instruction executes — the same cadence as the trace hook — attached
  through `Vm::with_fuel_sink` and recovered through
  `Vm::take_fuel_sink`; a fuel-refused dispatch never executes and
  never ticks (the fault stream owns the refusal). `FuelCollector`
  captures the diary in bounded memory (`bounded(max)` + a displayed
  overflow flag) and folds as it captures: `burn_by_function()` — the
  per-function dispatch counts in a `BTreeMap`, deterministic by
  construction — and `last_tick()`, the last allowed dispatch, which
  for a budget that ran out names the site the refusal happened at.
  `render_fuel` renders the deterministic report (the header with the
  final remaining, the per-function burn lines, one line per tick, the
  explicit `no fuel ticks` empty state). Contract like the other
  seams: read-only, total over the executed dispatches, zero-cost when
  unused (one `Option` check per dispatch), and composing with
  everything else (the tick count equals the trace's step count on one
  run). A bench (`benches/vm_fuel`: the per-dispatch checkpoint hook's
  overhead vs the unattached baseline over a budget-burning countdown
  loop, and the render) and a consumer walkthrough
  (`examples/fuel_walkthrough`: the exact-fit budget's diary, the
  burn folds, the exhausted budget's how-far-it-got report, the
  unbounded run's `None`-remaining ticks, the bounded capture, and the
  read-only proof) land with it.
- **Operand-stack observation seam (`glyim_bytecode_vm::stack`)**: the
  *movement counterpart* of the value seams — the watch seam records
  what values slots *took* and the trace seam where the machine went,
  but the operand stack, the channel every opcode's operands and
  answers actually travel through, was observable only as a depth
  number (`VmStep::stack_depth`); a `VmError::BinOpTypeMismatch` names
  the kinds that collided, but a *silent* wrong answer — a binop that
  consumed a neighbouring kind and produced a plausible `Int` — left
  nothing behind. The VM now hands one `Pushed` (the site, the
  producing dispatch number, the frame depth, the depth the movement
  produces, and the value in full as a *sink-side copy*) to a
  `StackSink` just before a value lands on the operand stack and one
  `Popped` just after a pop resolves, attached through
  `Vm::with_stack` and recovered through `Vm::take_stack`; every push
  and pop funnels through the machine's own `push_operand`/`pop` paths
  (the push funnel is the seam's wire-up — the thirteen raw
  `self.stack.push` sites now all route through one hook point), so
  the stream is total over the executed movements: the call's argument
  and target pops, the binop's right-then-left operand pops, the
  aggregate folds, the retval pops, the drops. A failed pop — a
  `VmError::StackUnderflow` — pops nothing and reports nothing (the
  fault stream owns the refusal), and a unit-return's empty-stack pop
  is a failed pop whose substituted `Unit` never rides the stack (the
  call seams' events cover the dest write). `StackCollector` captures
  the interleaved transcript (`events()` plus filtered
  `pushed_events()`/`popped_events()` views) in bounded memory
  (`bounded(max)` + a displayed overflow flag) and folds as it
  captures: `pushed_count()`/`popped_count()` plus `net_growth()` —
  the run's balance (`0` for every program that ends as it started) —
  and `max_depth_seen()`, the operand-stack high-water mark as the
  machine itself shaped it. `render_stack` renders the deterministic
  listing (the header alone for the empty capture; one `push`/`pop`
  line per movement with the watch render's value spellings — floats
  carry their bit pattern, pool strings stay quoted, tuples render
  recursively). `Value::kind_name` is public now (the same
  documented-kind surface the casts' and watch's kinds already
  expose), so the tracing sink's movement logs spell the moved kind.
  Contract like the other seams: read-only, total over the executed
  stream, zero-cost when unused (one `Option` check per movement, the
  value copies paid only on the fire path), and composing with
  everything else. A bench (`benches/vm_stack`: the per-movement
  funnel hooks' overhead vs the unattached baseline over a
  movement-heavy countdown loop — twelve movements per iteration — and
  the render) and a consumer walkthrough
  (`examples/stack_walkthrough`: the golden stream, the balance folds,
  the movement-heavy loop, the bounded capture, the silent failed pop,
  the silent unit return, and the read-only proof) land with it.
- **Float-operation observation seam (`glyim_bytecode_vm::floats`)**:
  the *arithmetic counterpart* of the value seams — every opcode class
  carried an observation hook except the float stream, and the float
  stream is the only arithmetic the machine runs that *never faults*:
  integer division by zero traps, but `1.0 / 0.0` is the documented
  convention (`+inf`, matching LLVM's `fdiv` and the MIR interpreter),
  and `inf - inf` or `0.0 * inf` land as NaN — a silently-poisoned
  value that flows into comparisons (every NaN compare is false) and
  reads like a lowering bug rather than the arithmetic it came from.
  The VM now hands one `FloatConst` (the site and the constant) per
  executed `OP_LOAD_CONST_F64` just after the payload has been decoded
  and before the value is pushed, one `FloatBinop` (the site, the
  opcode, the operands in full as *sink-side copies*, and the answer —
  a `Float` for arithmetic, the machine's `Int` bool encoding for
  comparisons) per executed `(Float, Float)` binop just after the arm
  has computed its answer — a *refused* pairing (mixed `Int`/`Float`
  kinds, a bitwise op over floats) faults with
  `VmError::BinOpTypeMismatch` and never reaches the hook (the fault
  stream owns the refusal) — and one `FloatNeg` (the site, the
  operand, and the IEEE negation) per executed `OP_NEG` over a float,
  to a `FloatSink` attached through `Vm::with_floats`; the constant
  pushes that executed before a refusal still report (the seam is
  total over the *executed* float stream). `FloatCollector` captures
  the interleaved transcript (`events()` plus filtered
  `const_events()`/`binop_events()`/`neg_events()` views) in bounded
  memory (`bounded(max)` + a displayed overflow flag) and folds as it
  captures: `const_count()`/`binop_count()`/`neg_count()` plus the
  IEEE-754 specials folds — `nan_results()` and `inf_results()`, the
  arithmetic answers that came back NaN or infinite, the convention
  path kept visible instead of silently consumed. `render_floats`
  renders the deterministic listing (the explicit `no float events`
  empty state; one `const`/`binop`/`neg` line per event with the
  machine's value spellings — the float spelling carries the bit
  pattern, so `-0.0` and the specials stay auditable). `Opcode::name`
  is public now, the same documented-mnemonic surface the casts' and
  watch's kinds already expose, so the binop lines spell the
  dispatched opcode. Contract like the other seams: read-only,
  total over the executed stream, zero-cost when unused (one `Option`
  check per float operation, the operand copies paid only on the fire
  path), and composing with everything else. A bench
  (`benches/vm_floats`: the per-float-op hooks' overhead vs the
  unattached baseline over a float-heavy countdown loop, and the
  render) and a consumer walkthrough (`examples/floats_walkthrough`:
  the one-binop kernel, both specials, the comparison and negation
  faces, the float-heavy loop, the bounded capture, the refused
  pairing, and the read-only proof) land with it. With this seam every
  behavioral opcode class carries an observation hook.
- **Composite-construction observation seam (`glyim_bytecode_vm::aggregates`)**:
  the *construction counterpart* of the value seams — the watch seam
  records what values slots *took*, the drop seam what the run
  *destroyed*, the cast seam what it *transformed*, but the moment of
  *construction* was invisible: the `OP_AGGREGATE` fold that turns the
  operand stack's top `n` values into one tuple, and the `OP_REPEAT`
  replication that turns one value and a count into an `n`-tuple,
  produced their tuples with no observation anywhere on the path, so a
  lowering bug that aggregated the wrong operand span (or repeated into
  the wrong length) shipped as a wrong tuple. The VM now hands one
  `AggregateBuilt` (the fold site, the arity, and the folded elements
  in full as *sink-side copies*) per executed `OP_AGGREGATE` just
  after the fold has popped its operands — an operand-stack underflow
  mid-fold faults and never executes, so it reports nothing (the fault
  stream owns the refusal) — and one `RepeatBuilt` (the site, the
  *raw* count, and the replicated value) per executed `OP_REPEAT` just
  after the count and value have been popped, to an `AggregateSink`
  attached through `Vm::with_aggregates`; a negative repeat count is
  not a fault (the machine clamps it to the empty tuple) and the event
  displays the raw count instead of silently pairing the clamp away.
  `AggregateCollector` captures the interleaved transcript (`events()`
  plus filtered `builds()`/`repeats()` views) in bounded memory
  (`bounded(max)` + a displayed overflow flag) and folds as it
  captures: `built_count()`/`repeat_count()`, `total_elements()` —
  every folded arity plus every produced repeat length, the run's
  composite-materialization count (a negative repeat's clamp
  contributes `0`) — and `max_arity()`, the arity high-water mark.
  `render_aggregates` renders the deterministic listing (the explicit
  `no aggregate events` empty state, one `build` line with its element
  spellings or one `repeat` line with its raw count, replicated value,
  and produced arity per event). Contract like the other seams:
  read-only, total over the *executed* construction stream, zero-cost
  when unused (one `Option` check per construction), and composing
  with everything else. A bench (`benches/vm_aggregates`: the
  per-construction hooks' overhead vs the unattached baseline over a
  repeat-heavy countdown, and the render) and a consumer walkthrough
  (`examples/aggregates_walkthrough`: the one-fold shape, the
  repeat-heavy loop, the negative-count clamp, the bounded capture,
  and the read-only proof) land with it.
- **Composite-projection observation seam (`glyim_bytecode_vm::projections`)**:
  the *interrogation counterpart* of the aggregate seam — the
  aggregate seam watches the moment of *construction*, but the
  machine's only aggregate-projection opcodes (`OP_LEN`, the length
  read over a local; `OP_DISCRIMINANT`, the discriminant read over the
  popped operand) carried no observation, and both have a documented
  *scalar convention* — a projection over a non-aggregate value
  answers `0` (the pre-float VM's convention) — so a lowering bug that
  projected a scalar, or a field-offset bug that read the wrong slot's
  length, landed in that convention as a silent zero. The VM now hands
  one `LenRead` (the read site, the local index, the projected operand
  in full as a *sink-side copy*, and the machine's answer) per
  executed `OP_LEN` just after the operand has been fetched from its
  local — an out-of-bounds local index faults and never executes, so
  it reports nothing (the fault stream owns the refusal) — and one
  `DiscrRead` (the read site, the popped operand, and the answer) per
  executed `OP_DISCRIMINANT` just after the operand has been popped,
  to a `ProjectionSink` attached through `Vm::with_projections`.
  `ProjectionCollector` captures the interleaved transcript
  (`events()` plus filtered `len_events()`/`discr_events()` views) in
  bounded memory (`bounded(max)` + a displayed overflow flag) and
  folds as it captures: `len_count()`/`discr_count()` and
  `scalar_reads()` — the scalar-convention tally, the path a lowering
  bug lands in, kept visible instead of silently consumed.
  `render_projections` renders the deterministic listing (the explicit
  `no projection events` empty state, one `len` line with its local
  index or one `discr` line per event, the operand in the watch
  render's value spellings, and the answer after a `->`). Contract
  like the other seams: read-only, total over the *executed*
  projection stream, zero-cost when unused (one `Option` check per
  read), and composing with everything else. A bench
  (`benches/vm_projections`: the per-projection hooks' overhead vs the
  unattached baseline over a projection-heavy countdown, and the
  render) and a consumer walkthrough (`examples/projections_walkthrough`:
  the length read, the discriminant read, the scalar convention, the
  projection-heavy loop, the bounded capture, and the read-only proof)
  land with it.
- **Call-stream observation seam (`glyim_bytecode_vm::calls`)**: the
  *call-ABI counterpart* of the call-graph and call-state seams — the
  call graph folds the execution trace into per-`(caller, callee)`
  edges and the call-state seam snapshots a frame's locals at its
  boundaries, but the trace hook fires *before* a call executes and its
  one-line step entries fold the call's operands away: the argument
  values, the destination local, and the returned value were unreadable
  after the fact. The VM now hands one `CallStarted` (the caller's
  function index, the call site, the callee, the argument values in
  full as *sink-side copies*, the destination local, and the resume
  target) per executed `OP_CALL`/`OP_CALL_INDIRECT` just after the
  callee's frame is pushed, and one `CallReturned` (the returning
  function, the frame depth it sat at, and the value it handed back as
  a sink-side copy) per executed `OP_RETURN` just after the frame is
  popped, to a `CallSink` attached through `Vm::with_calls` — the entry
  frame's return is the run's last event, its value the module result.
  `CallCollector` captures the interleaved transcript (`events()` plus
  filtered `starts()`/`returns()` views) in bounded memory
  (`bounded(max)` + a displayed overflow flag) and folds as it
  captures: `open_calls()` — the calls started but not yet returned,
  displayed by `render_calls` as `unreturned calls: N`, never silently
  paired away — `max_depth()`, and `indirect_count()`. Contract like
  the other seams: read-only, total over the *executed* call stream (a
  call the machine refuses — a bad function index, a depth overflow —
  never executes and reports nothing; the fault stream owns the
  refusal), zero-cost when unused (one `Option` check per call and per
  return), and composing with everything else. A bench
  (`benches/vm_calls`: the per-call hooks' overhead vs the unattached
  baseline over a call-heavy countdown, and the render) and a consumer
  walkthrough (`examples/calls_walkthrough`: the paired call and
  return, the bounded recursion's depth folds, the fault's open call,
  the call-heavy loop, the bounded capture, and the read-only proof)
  land with it.
- **String-pool observation seam (`glyim_bytecode_vm::strings`)**: the
  *runtime counterpart* of the module-image diff's structural pool
  lines — the diff names pool *changes* between two artifacts, but the
  pool's runtime face was invisible: which slots did this run resolve,
  from which sites, and how many times was one shared slot
  re-resolved (the shared-slot audit where an opaque-index constant or
  a content-equality break shows up as the wrong slot)? The VM now
  hands one `StringLoadEvent` (the resolution site, the pool slot, and
  the resolved byte string in full as a *sink-side copy*) per executed
  `OP_LOAD_CONST_STR` just after the slot's byte string is resolved
  and before the value is pushed, to a `StringSink` attached through
  `Vm::with_strings`. `StringCollector` captures the transcript in
  bounded memory (`bounded(max)` + a displayed overflow flag) and
  folds as it captures: `resolutions(slot)` and `slot_count()` — the
  per-slot counts — and `total_bytes()`, and `render_strings` renders
  the deterministic listing (the explicit `no string loads` empty
  state, one site/slot/value line per load in the watch render's value
  spellings, then the per-slot summary in slot order). Contract like
  the other seams: read-only, total over the *executed* resolutions (a
  bad pool index faults and resolves nothing — the fault stream owns
  the refusal), zero-cost when unused (one `Option` check per load),
  and composing with everything else. A bench (`benches/vm_strings`:
  the per-resolution hook's overhead vs the unattached baseline over a
  string-heavy countdown, and the render) and a consumer walkthrough
  (`examples/strings_walkthrough`: the two-slot shape, the shared-slot
  loop, the bounded capture, and the read-only proof) land with it.
- **Memory-observation seam (`glyim_bytecode_vm::memory`)**: the
  *frame-memory counterpart* of the local-watch and data-flow seams —
  the watch records what values a module's local slots *took* and the
  data-flow seam pairs those writes with the reads that consumed them,
  but both watch the *slot* face. The aggregate lowering also runs on
  the *memory* face: `OP_STORE_FIELD` writes a field through an address
  and `OP_DEREf` reads it back, and the frame's addressable memory
  region is zero-initialised — a read of an address the run never wrote
  is a silent zero, not an error, so the wrong-field-offset class (read
  a neighbour slot's zero instead of the stored field) was invisible
  after the run. The VM now hands one `MemWriteEvent` (the store site:
  function index and the opcode's byte offset; the frame-relative
  address; the stored value in full) per executed `OP_STORE_FIELD` just
  after the store lands, and one `MemReadEvent` (the deref site, the
  address, the fetched value) per executed `OP_DEREf` just after the
  value is fetched, to a `MemSink` attached through `Vm::with_memory` —
  both events take *sink-side copies* of their values (the read hook's
  copies-into-the-event shape), so the machine's own values stay exactly
  where they were. `MemCollector` captures the interleaved transcript
  (`events()` — writes and reads in execution order — plus filtered
  `writes()`/`reads()` views) in bounded memory (`bounded(max)` + a
  displayed overflow flag) and folds as it captures: `is_written(addr)`
  and `unwritten_reads()`/`unwritten_read_count()` — the reads whose
  address had no prior write when the read executed — and
  `render_memory` renders the deterministic listing (the explicit
  `no memory events` empty state, one kind/site/address/value line per
  access in the watch render's value spellings, then the unwritten reads
  as their own section). Contract like the other seams: read-only (the
  stored and fetched values are unchanged — the sink observes, it never
  rewrites memory), total over the *executed* accesses (a store refused
  by the bounds check writes nothing and reports nothing — the fault
  stream owns the refusal, the watch seam's failed-store rule),
  zero-cost when unused (one `None` check per executed access), and
  composing with everything else. A bench (`benches/vm_memory`: the
  per-access hooks' overhead vs the unattached baseline over a
  memory-heavy countdown, and the render) and a consumer walkthrough
  (`examples/memory_walkthrough`: the paired store-and-read, the
  unwritten read, the memory-heavy loop's per-iteration traffic, the
  bounded capture, and the read-only proof) land with it.
- **Cast-observation seam (`glyim_bytecode_vm::casts`)**: the
  *value-transformation counterpart* of the local-watch and data-flow
  seams — the watch records what values a module's local slots *took*
  and the data-flow seam pairs those writes with the reads that consumed
  them, but every `OP_CAST` *changes a value in place* and the change
  vanished from every view at the push: the execution trace names the
  cast's kind and target bytes, yet the operand it consumed and the
  result it produced are folded away by the one-line step entries, and
  the run's own output shows only the *last* cast's result. The classic
  silent-lossy-cast audit — `(-1i32) as u8 == 255`, `(3.99) as i32 == 3`
  — was unreadable after the fact. The VM now hands one `CastEvent` (the
  cast site: function index and the opcode's byte offset; the decoded
  kind/target pair — the seam's own `CastKind` spelling of the emitter's
  cast-kind byte, named the way the typed cast-mismatch errors spell it;
  the operand/result values in full) to a `CastSink` attached through
  `Vm::with_casts` the moment a cast has produced its result, just
  before the result is pushed — the event takes *sink-side copies* of
  both values (the read hook's copies-into-the-event shape), so the
  machine's own values stay exactly where they were and the seam adds
  one clone per traced cast. `CastCollector` captures the stream in
  bounded memory (`bounded(max)` + a displayed overflow flag, the
  honesty rule every other collector follows) with `events_of(func)`
  site reads, `events_of_kind(kind)` kind reads (the float-cast audit's
  first query), and `changed_count()` (the structural-equality fold:
  every cross-kind cast and every truncation/wrap counts, an identity
  cast does not), and `render_casts` renders the deterministic listing
  (the explicit `no casts` empty state, one kind/target site line per
  cast with the operand/result pair in the watch render's value
  spellings — floats carry their raw bits, so the golden is
  platform-exact). Contract like the other seams: read-only (the pushed
  result is unchanged — the sink observes, it never rewrites a cast's
  outcome), total over the *executed* casts (a cast that faults —
  `VmError::UnknownCastKind` / `VmError::CastOperandMismatch` — never
  executes, so it reports nothing; the fault stream owns it), zero-cost
  when unused (one `None` check per executed `OP_CAST`), and composing
  with everything else. A bench (`benches/vm_casts`: the per-cast hook's
  overhead vs the unattached baseline over a cast-heavy countdown, and
  the render) and a consumer walkthrough (`examples/casts_walkthrough`:
  the wrap, the truncation, the identity, the float-cast kind read, the
  cast-heavy loop's per-iteration stream, the bounded capture, and the
  read-only proof) land with it.
- **Drop-observation seam (`glyim_bytecode_vm::drops`)**: the
  *destruction-side counterpart* of the local-watch and call-state seams
  — the watch records what values a module's local slots *took* and the
  call-state seam records what frames *held*, but every `OP_DROP`
  discards a value and the discarded value vanished from every view at
  the pop, so the emitter's scope-exit shape (one `OP_DROP` per destroyed
  value) was invisible after the run. The VM now hands one `DropEvent`
  (the discard site: function index and the opcode's byte offset; the
  discarded value in full) to a `DropSink` attached through
  `Vm::with_drops` at each executed `OP_DROP` — the event *takes* the
  value the opcode already popped, so the seam adds no clone on the fire
  path. `DropCollector` captures the stream in bounded memory
  (`bounded(max)` + a displayed overflow flag, the honesty rule every
  other collector follows) with `events_of(func)` site reads, and
  `render_drops` renders the deterministic listing (the explicit
  `no drops` empty state, one site-plus-value line per discard in the
  watch render's value spellings — integers plain, floats with their raw
  bits, strings quoted, tuples parenthesized). Contract like the other
  seams: read-only (the pop the opcode pays is unchanged — the sink
  observes, it never resurrects a discarded value), total over the
  *executed* discards (a refused dispatch never discards, so a fuel
  budget's last cut reports nothing), zero-cost when unused (one `None`
  check per executed `OP_DROP`), and composing with everything else (the
  traced, budgeted, assert-observed run's discard stream is unchanged by
  the other hooks). A bench (`benches/vm_drops`: the per-discard hook's
  overhead vs the unattached baseline over a drop-heavy countdown, and
  the render) and a consumer walkthrough (`examples/drops_walkthrough`:
  the destroyed aggregate and the discarded address, the drop-heavy
  loop's per-iteration stream, the bounded capture, and the read-only
  proof) land with it.
- **Assertion-observation seam (`glyim_bytecode_vm::asserts`)**: the
  *invariant counterpart* of the coverage and branch-history seams — an
  `OP_ASSERT` is a decision the program's own invariant wrote, but its
  outcome is folded away by both views (coverage counts the site as hit,
  the branch history records the landing the polarity forced), and a
  failing run's `VmError::AssertionFailed` answers "which assert failed"
  only for the *last* one — nothing said whether the run's earlier
  assertions actually held. The VM now hands one `AssertEvent` (the site:
  function index and the opcode's byte offset; the asserted polarity; the
  observed condition; the pass/fail verdict) to an `AssertSink` attached
  through `Vm::with_asserts` the moment an assert's condition has been
  compared against its asserted polarity, *before* the fault a failing
  assert raises — so the failing assert is the stream's last entry, the
  run's earlier passes never silently vanish behind it. `AssertCollector`
  captures the stream in bounded memory (`bounded(max)` + a displayed
  overflow flag, the honesty rule every other collector follows), with
  `passed_count`/`failed_count` tallies, `events_of(func)` site reads,
  and a deterministic `render_asserts` listing (the explicit
  `no assertions` empty state, one `pass`/`fail` line per evaluation in
  the machine's Int-bool spelling). Contract like the other seams:
  read-only (the verdict never flips, a failing assert faults with
  exactly the pre-seam error), total over the *evaluated* asserts (a
  refused dispatch never evaluates, so a fuel budget's last cut reports
  nothing), zero-cost when unused (one `None` check per executed
  `OP_ASSERT`), and composing with everything else (the traced,
  budgeted, windowed run's assert stream is unchanged by the other
  hooks). A bench (`benches/vm_asserts`: the per-assert hook's overhead
  vs the unattached baseline over an assert-heavy countdown, and the
  render) and a consumer walkthrough
  (`examples/assertions_walkthrough`: the passing assert, the
  assert-not polarity shape, the failing assert's displayed verdict,
  the bounded capture, and the read-only proof) land with it.
- **Data-flow seam (`glyim_bytecode_vm::dataflow`)**: the *read-side
  counterpart* of the local-watch seam — the watch records what values a
  module's local slots took, but nothing paired those writes with the
  reads that consumed them, and a run that reads a slot it never stored
  in (the frame's zero-initialisation leaking into semantics) was
  invisible to every other seam: the trace shows a `LoadLocal`, the watch
  shows the writes, but "this read had no producing write in its frame"
  was nowhere. The VM now hands one `ReadEvent` (the watch event's field
  shape minus the write kind: function, slot, value, site, lifetime
  dispatch number, frame depth) to a `ReadSink` attached through
  `Vm::with_reads` for every successful `OP_LOAD_LOCAL`, and
  `DataFlowCollector` — a shared-state fold implementing *both* sink
  traits, attached through `Vm::with_watch` *and* `Vm::with_reads` (the
  hooks take the sink by value, so the collector's clone shares one
  fold) — links each read to the dispatch that last wrote its slot per
  `(function, frame depth, slot)`, the same key the watch events carry,
  so recursion disambiguates by depth. A read whose slot no write in its
  frame ever defined is reported through `uninitialized_reads` — the
  zero-init consumption is a *displayed* fact, never a silent one — with
  the displayed list capped (`with_uninitialized_cap`) and the exact
  total kept beside it (the same honesty rule as the bounded collectors'
  overflow flags). `FlowRecorder` captures the interleaved write/read
  transcript in arrival order (bounded or unbounded) and
  `DataFlowCollector::from_transcript` replays it into the identical
  fold; `TracingReadSink` emits one `tracing::debug!` event per read for
  the workspace-standard observability layer; `render_data_flow` renders
  the deterministic listing (the explicit `no data-flow events recorded`
  empty state, slot lines sorted by `(function, depth, slot)`, the
  per-slot `last def` dispatch). Contract like the other stream seams:
  read-only, total over the *successful* reads (a faulting bounds load
  reads nothing and reports nothing; the address-form
  `OP_LOAD_LOCAL_ADDR` and the aggregate-length `OP_LEN` are not value
  reads and are out of scope, the same scoping the watch seam applies to
  its write paths), zero-cost when unused (one `None` check per local
  read), and composing with everything else (fuel refusal means a read
  never happens, a decoded image reads exactly like an in-memory module,
  and the trace ride-along is unchanged). A bench
  (`benches/vm_dataflow`: the paired fold's per-read overhead over a
  read-heavy countdown, the transcript capture, the render) and a
  consumer walkthrough (`examples/data_flow_walkthrough`: the ABI pair,
  the interleaved transcript, and the uninitialized-read report) land
  with it.
- **Cache-observability seam (`glyim_db::cache_metrics`)**: the
  *lifecycle counterpart* of the compile cache's best-effort contract —
  misses, stores, and budget evictions were deliberately silent (nothing
  must be reported), but that silence made an unexpectedly slow rebuild
  indistinguishable from a rebuild that recompiled everything, and an
  eviction that pushed a hot object out of the size-capped cache was
  invisible until the next full recompile. The cache now hands one
  `CacheEvent` (`Hit` / `Miss` / `Stored` / `Evicted`, each with the
  object's size where bytes move) to the attached `CacheSink` just after
  each transition completes — the same sink/collector/render shape the
  bytecode VM's seam family uses. `CacheCollector` folds the stream into
  an O(1) aggregate (counts and byte totals plus the last key of each
  kind — memory is a constant, never a function of how many compiles
  ran) with a `from_events` transcript re-fold and a `hit_rate()` that
  is `None` when no lookup ran (an average over nothing is not a fact);
  `render_cache_report` renders the deterministic listing (the explicit
  `no cache events recorded` empty state, the `n/a (no lookups)`
  pure-writer state); `CacheTracingSink` emits one `tracing::debug!`
  event per transition for the workspace-standard observability layer.
  Wire-up mirrors the VM: `CompileCache::with_sink` attaches,
  `CompileCache::take_sink` takes the observer back out (the
  `Vm::take_trace` shape, downcast through the `as_any` hook), and the
  new `CompileCache::with_max_bytes` pins the budget explicitly (the
  builder wins over the `GLYIM_CACHE_MAX_BYTES` environment default —
  what deterministic tests need). Contract like the other seams:
  read-only, total (a failed store reports nothing because nothing was
  stored; the empty-file guard reports as a miss, because the caller
  pays a full compile either way), and zero-cost when unused (one
  lock-and-check per transition). The seam also retires the eviction
  trim's dead `removed` accumulator — its bytes now travel on the
  `Evicted` events — and wires the orphaned `glyim-db` `src/tests`
  module (three `Database` tests that sat undeclared in `lib.rs` and so
  never compiled or ran) back into the test tree. A bench
  (`benches/cache_metrics`: the 64k-event fold, the render, and the
  sink-attached vs bare per-lookup overhead) and a consumer walkthrough
  (`examples/cache_metrics_walkthrough`: miss → store → hit → eviction,
  rendered) land with it.
- **Tail-window seam (`glyim_bytecode_vm::window`)**: the
  *bounded-memory failure counterpart* of the execution-trace seam —
  the trace records every dispatch (the right answer at O(steps)
  memory) and the fault-backtrace seam names the fault site, but
  nothing answered the flight-recorder question: what did the run's
  last dispatches look like immediately before it ended? A
  `WindowCollector` — a fixed-size ring over the same per-dispatch
  stream the trace hook produces — holds the most recent `n` steps
  regardless of the run's length (the memory bound is the window, never
  the transcript), with the newest entry always the run's final
  dispatch: the hook fires *before* the instruction executes, so a
  faulting `Call` and a fuel budget's last allowed dispatch land in the
  tail. The dispatches the bound scrolls out are counted in
  `WindowCollector::dropped` — a *displayed* fact, never a silent drop
  (the same honesty rule as the call-state seam's overflow flag), and
  `first_dispatch()` names the window's absolute 1-based starting
  dispatch so the tail slots into the run's numbering. Contract like
  the other stream folds: read-only, total, bounded, deterministic —
  `render_window` spells each captured dispatch in the trace render's
  exact field shape (the payload spelling is now shared
  `trace::render_detail`), with the explicit
  `no dispatches in the window` line, and a `from_tail` re-fold of a
  captured transcript. A bench (`benches/vm_window`: the ring's
  dispatch overhead vs the untraced baseline over a countdown loop, the
  post-hoc re-fold, the render) and a consumer walkthrough
  (`examples/fault_window_walkthrough`) land with it.
- **Inclusive-cost seam (`glyim_bytecode_vm::cost`)**: the *call-tree*
  counterpart of the execution-profile seam — profile charges every
  dispatch to the function that executed it, so a run spent inside a
  callee reads as "one dispatch here, a million there" and the
  *cumulative* question ("which call subtree dominates the run?") has no
  answer in any flat column. `CostCollector` folds the same per-dispatch
  stream the trace hook produces into per-function **inclusive** and
  **exclusive** dispatch counts: the fold reconstructs the active call
  stack from the stream's `frame_depth` transitions (the call-graph
  seam's follow-step trick in its depth edition — no `Call` payload is
  needed, so `OP_CALL` and `OP_CALL_INDIRECT` fold identically and
  recursion disambiguates by depth), and every active frame earns one
  inclusive credit per dispatch while the executing function alone earns
  the exclusive credit (a function active at several depths of a
  recursion earns one credit per active frame, so the inclusive column
  sums the whole active subtree). Contract like the other stream folds:
  read-only, total (a faulting dispatch and a fuel budget's last allowed
  dispatch fold in — the entry frame is on the stack for the whole run,
  so the root's inclusive count always equals the step total), bounded
  (two counters per observed function plus the active stack — memory is
  a function of the module's shape, never of the run length), and a
  deterministic `render_cost` listing (the `fn <i>: inclusive=<a>
  exclusive=<b>` lines over every function of the module, the
  hottest-five summary, the explicit `no dispatches` line) with a
  `hottest_inclusive(n)` ranking (ties by function index ascending) and
  a `from_steps` re-fold of a captured transcript. A bench
  (`benches/vm_cost`: the live fold's dispatch overhead vs the untraced
  baseline over a call-heavy loop, the post-hoc re-fold, the render) and
  a consumer walkthrough (`examples/inclusive_cost_walkthrough`) land
  with it.
- **Call-state seam (`glyim_bytecode_vm::state`)**: the *frame-lifecycle*
  counterpart of the local-watch seam — watch reports one event per
  local *write*, but nothing answered the frame-shaped question: at the
  moment a call began, what did the callee's locals hold (which arguments
  landed where, was the return place still zero), and at the moment it
  returned, what did the frame hold after everything the body did? A new
  `StateSink` — the first hook the VM fires at the frame *lifecycle*
  rather than per dispatch — hands one `StateEvent` per boundary from two
  sites: `Vm::push_frame` fires `frame_enter` the moment a frame's locals
  are bound (the MIR ABI's zero return place at slot 0, the arguments at
  `1..=arg_count`), and the `OP_RETURN` arm fires `frame_exit` with the
  frame's final local state before the frame is discarded (the entry
  frame's exit is the run's last event — a straight-line run reports
  exactly two boundaries). Each event carries the frame's function index
  and 1-based depth, so a recursion's nesting reads LIFO, and a full
  locals snapshot paid only when a sink is attached (one `Option` check
  per frame boundary when none is — the pre-seam behavior). Contract
  like the other seams: read-only, total, bounded (`StateCollector::bounded`
  flags overflow instead of growing without limit over unbounded
  recursion — a displayed fact, never a silent drop), deterministic
  `render_state` listing (the `enter`/`exit` phases, the `fn`/`depth`
  pair, the locals in `render_watch`'s value spellings, the explicit
  `no frame boundaries` line), and an honest unbalanced tail — a fuel
  budget or a fault ending the run between the boundaries is displayed,
  never silently paired away. A bench (`benches/vm_state`: the
  frame-boundary hook's overhead vs the unattached baseline over a
  call-heavy loop — one locals snapshot per boundary — and the render)
  and a consumer walkthrough (`examples/call_state_walkthrough`) land
  with it.
- **Branch-history seam (`glyim_bytecode_vm::branches`)**: the
  *directional* counterpart of the instruction-coverage seam — coverage
  answers which instruction sites a run dispatched and which stayed dead,
  but it cannot say which way a branch actually went (an untaken branch
  and a taken one are equally "covered" sites). `BranchCollector` folds
  the same per-dispatch stream the trace hook produces into
  per-decision-site histories keyed `(function, pc)`: for a `JumpIf`, the
  taken/fall-through split named by the step that *follows* the branch
  (the machine's next dispatch — the stream carries no "branch was taken"
  bit, so the fold resolves outcomes the same way the call-graph seam
  names a callee by the following step), and for every decision opcode a
  landing map of where control went next, with per-outcome counts.
  `Jump`, `SwitchInt`, and `Assert` transfer unconditionally on success,
  so their landing map is the whole outcome (a `SwitchInt`'s arm values
  travel in the instruction payload, not the stream — the landing map is
  the honest observable); a held `Assert` lands on its target, a failed
  one ends the run in `VmError::AssertionFailed`. A decision whose
  resolving step never arrives (a fault at the branch itself — a float
  condition under `JumpIf` — a fuel budget cut between the branch and its
  landing, the branch as the stream's last dispatch) keeps its dispatch
  count at its site and reports through `unresolved_decisions` —
  displayed, never silently dropped. Deterministic `render_branches`
  listing (site lines with the disassembler's mnemonics, landing lines in
  ascending key order, the explicit `no decision dispatches` line) and a
  `from_steps` re-fold of a captured transcript, exactly like the other
  stream folds. A bench (`benches/vm_branches`: the live fold's dispatch
  overhead vs the untraced baseline over a branch-heavy loop, the
  post-hoc re-fold, the render) and a consumer walkthrough
  (`examples/branch_history_walkthrough`) land with it.
- **Run-diff seam (`glyim_bytecode_vm::rundiff`)**: the *behavioral*
  counterpart of the module-image diff — `render_module_diff` answers
  what changed between two decoded *artifacts*, but nothing answered
  what changed between two *runs* (an emitter tweak that leaves the
  artifact bytes almost untouched can still change what a run does, and
  reconstructing the behavioral delta from two traces means eyeballing
  step lists side by side). `render_run_diff` walks two captured
  transcripts ([`VmStep`] lists, as captured by `TraceCollector`)
  dispatch by dispatch and renders where the two runs part ways
  deterministically: the `diverge at dispatch N:` site with both steps
  spelled in `render_trace`'s field shape (site, depths, mnemonic —
  unknown bytes as the disassembler's `unknown(0xNN)` — and the payload,
  module-free so the two sides may come from different modules: a
  `ConstStr` step shows the pool index and its resolution, never the
  content) plus the honest tail counts, or the shorter side's explicit
  end line when one transcript is a strict prefix of the other (a fuel
  budget or a fault ends one run where the other continues — an end is
  an end, never a divergence, never a silent pad). Two identical
  transcripts render the explicit `no divergence` line — the
  determinism gate any no-op change must pass (the seam's test pins two
  real runs of the same module and walks an emitter tweak that changes
  one constant). The diff is a pure function over the two transcripts,
  total over any pair (the empty pair included). A bench
  (`benches/vm_run_diff`: the identical pair's full scan, the
  early-divergence short circuit, the render) and a consumer walkthrough
  (`examples/run_diff_walkthrough`) land with it.
- **Call-graph seam (`glyim_bytecode_vm::callgraph`)**: the
  *structural-dynamic* counterpart of the VM's observability suite — the
  profile answers where the dispatches went by opcode and function, the
  coverage which instruction sites ran, but none of them answered who
  *called whom* (a hot function's dispatch count says nothing about
  whether one caller fans out to it or five do; reconstructing edges from
  a trace means replaying the frame stack by hand). `CallGraphCollector`
  impls `TraceSink` and folds the per-dispatch stream into
  per-`(caller, callee)` edge dispatch counts in bounded memory (a
  `BTreeMap` keyed by function-index pairs — the render's order is the
  map's order, never an iteration accident). Because the trace hook fires
  *before* an instruction executes and the `Call` wire payload carries the
  arity, the destination local, and the resume target — not the callee
  (the callee index sits on the operand stack, invisible to a read-only
  sink) — the fold names the callee by the step that *follows* the call:
  a successful call dispatches into the callee's entry instruction, whose
  step sits exactly one frame deeper, so recursion disambiguates by depth
  and a self-edge accumulates once per recursive dispatch. A call whose
  callee never dispatches (an out-of-range function index, a faulting
  entry instruction, a callee with empty code, a fuel budget cut between
  the call and the entry) stays in `calls()` and reports through
  `unresolved_calls()` — displayed, never silently dropped. The seam is
  read-only (the watched result equals the untraced one), total over the
  folded stream (the faulting dispatch of a failed run is folded in), and
  deterministic (`render_call_graph`'s listing is pinned by a golden
  test: the header, the ascending edge lines, the explicit `no calls`
  line for a call-free run, and the `unresolved-calls` count). `from_steps`
  re-folds a captured `TraceCollector` transcript into the identical map
  post-hoc, the hook composes with every other seam (fuel ends the graph
  at the executed dispatches), and `callees_of`/`callers_of` read the map
  back in ascending order. The fold is one depth check per dispatch plus
  one map update per call. A bench (`benches/vm_callgraph`: the edge
  fold's dispatch overhead vs the untraced baseline over a call-heavy
  loop, the `from_steps` re-fold, the render) and a consumer walkthrough
  (`examples/call_graph_walkthrough`) land with it.
- **Module-image diff seam (`glyim_bytecode_vm::diff`)**: the artifact-level
  change report — the image seam ships a module across a process boundary
  and the disassembler lists what one artifact carries, but nothing
  answered what *changed* between two builds (the only prior option was a
  manual byte-by-byte eyeball, or `cmp`, which reports one offset and
  stops). `render_module_diff` walks two decoded [`Module`]s index by
  index — functions (code with the first-diff pc and the metadata pairs,
  block tables), the string pool, the entry point — and renders the
  differences deterministically: `-`/`+`/`~` lines per removed/added/
  changed function in index order (a same-index divergence is one change
  line, never a removal+addition pair; a prefix-truncated code buffer
  diffs at the shorter buffer's end), the same three shapes over the
  pool entries, and an explicit `entry: a -> b` line when the entry index
  moved. Identical modules render the explicit `no differences` line —
  the regression gate any emitter change that should not alter shipped
  bytes must pass (encode → decode → diff over the loaded pair, pinned by
  a round-trip test). The diff is a pure function over the two modules,
  total over any pair. A bench (`benches/module_diff`: the identical fast
  path and the divergent render, both sides riding the image round trip)
  and a consumer walkthrough (`examples/module_diff_walkthrough`) land
  with it.
- **Local-watch seam (`glyim_bytecode_vm::watch`)**: the *data-flow*
  counterpart of the VM's observability suite — the trace answers where a
  run went step by step, the profile where the dispatches aggregated, the
  coverage which instruction sites ran, the backtrace why a run failed,
  but none of them answered what a program's locals *held* (an
  `AssertionFailed` over a countdown loop says nothing about the counter's
  iteration-by-iteration values; reconstructing that from a trace means
  replaying the operand stack by hand). `Vm` grows a
  `with_watch(Box<dyn WatchSink>)` hook fired once per successful local
  write, covering all three write paths the machine has: the program's
  `OP_STORE_LOCAL` stores, the `OP_CALL` argument binds (the MIR ABI's
  arguments landing in the callee's locals `1..=arg_count`), and the
  callee's `OP_RETURN` landing its value in the caller's recorded
  `dest_local`. Each `WatchEvent` carries the writer function, the slot,
  the value (a clone — the machine keeps executing), the producing
  dispatch's site (`pc`) and 1-based lifetime number, the owning frame's
  depth (recursion disambiguated), and the `WatchVia` kind
  (`store-local` / `arg-bind` / `callee-return`). The seam is read-only
  (the watched result equals the unwatched one), total over the
  successful writes (a bounds-failing store writes nothing and reports
  nothing; zero-initialisation is allocation, not a write), bounded
  (`WatchCollector::bounded` marks overflow instead of growing without
  limit), and deterministic (`render_watch`'s listing is pinned by a
  golden test; values render in the disassembler's spellings — floats
  with their raw IEEE-754 bits, tuples parenthesized and recursive).
  `slot_history` reads one watched slot's values back in write order, and
  the hook composes with every other seam (trace + watch on one run, fuel
  ending the stream at the executed writes, the image round trip watching
  a decoded artifact identically). The hook is one `None` check per local
  write when no sink is attached. A bench (`benches/vm_watch`: the
  per-write capture overhead vs the unwatched baseline over a store-heavy
  countdown, and the render) and a consumer walkthrough
  (`examples/local_watch_walkthrough`) land with it.
- **Fault-backtrace seam (`glyim_bytecode_vm::backtrace`)**: the
  *failure-time* counterpart of the trace/profile story — the trace
  answers what a run did step by step, but when a loaded artifact *faults*
  the bare `VmError` carries no location (an `AbnormalTermination` says
  nothing about where or how deep, and reconstructing it from a trace
  means attaching a collector and scanning the transcript). `Vm` grows a
  `with_backtrace(Box<dyn FaultSink>)` hook fired exactly once when a run
  ends in a `VmError`, handing over a `FaultReport`: the fault site
  (function, dispatch pc, decoded opcode), the suspended caller chain
  (innermost first, a Rust-style backtrace, each frame at its resume pc),
  the lifetime dispatch count, and the fuel state. The seam is
  failure-only (a clean run never fires it), read-only (the caller
  receives the same error either way), and total (every error path fires —
  a pre-execution fault reports the entry request with zero dispatches, an
  unknown opcode byte keeps `opcode: None`, the fuel gate's refused
  dispatch is the reported site, and a cross-seam test pins the fault site
  to the trace's last recorded step). `BacktraceCollector` captures the
  report (a run ends at its first fault, so one captured report is the
  whole story) and `render_backtrace` renders it deterministically in the
  disassembler's diagnostic style (a golden test pins the trapped-callee
  listing end to end). The machine's current site is two integer stores
  per dispatch; the hook itself is one `None` check per *failed* run. A
  bench (`benches/vm_backtrace`: the clean-run baseline pair, the
  fault-path capture, the render, and the 1023-caller deep chain) and a
  consumer walkthrough (`examples/fault_backtrace_walkthrough`) land with
  it.
- **Instruction-coverage seam (`glyim_bytecode_vm::coverage`)**: the
  *spatial* aggregate over the trace stream — the profile answers where
  the dispatches went by opcode and function, the coverage answers which
  of the artifact's instruction sites ran and which are dead (an untaken
  branch, an unreachable block). `CoverageCollector` impls `TraceSink` and
  folds the per-dispatch stream into a per-`(function, pc)` hit map,
  aligned against the module's instruction-site inventory (the same walk
  `render_module`'s listing uses — one wire-format reading shared by the
  static listing and the dynamic map, so a covered site reads identically
  in both) in O(#sites) memory (bounded, never run-length).
  `render_coverage` renders the run deterministically: covered sites in
  module order with their hit counts (the disassembler's listing
  spellings), the `uncovered` dead-code list, and — over a corrupt block
  table that dispatched mid-instruction — a separate `misaligned hits`
  section that keeps the exact pc (displayed, never dropped, never counted
  against the aligned denominator). `from_steps` re-folds a captured
  `TraceCollector` transcript into the identical map post-hoc, a
  fuel-refused dispatch never reaches the hook (the coverage ends exactly
  at the executed sites), and the faulting dispatch of a failing run is
  folded in. A bench (`benches/vm_coverage`: the hit-map fold's dispatch
  overhead vs the untraced baseline, the `from_steps` fold, the render)
  and a consumer walkthrough (`examples/coverage_walkthrough`) land with
  it.
- **Execution trace seam (`glyim_bytecode_vm::trace`)**: the *dynamic* half
  of the image seam's observability story — the disassembler lists what a
  decoded artifact *carries*, the trace answers what it *did*. `Vm` grows a
  `with_trace(Box<dyn TraceSink>)` hook fired once per instruction dispatch
  (just before the instruction executes, so a faulting dispatch is the
  trace's last entry; unknown opcode bytes record as `opcode: None`), with
  two in-crate sinks: `TraceCollector` (bounded or unbounded capture — a
  bounded collector sets a displayed `overflowed` flag instead of growing
  without limit over an infinite loop) and `TracingSink` (one
  `tracing::debug!` event per dispatch, the workspace-standard layer). The
  step record (`VmStep`) carries function/pc/frame-depth/stack-depth plus a
  structured `StepDetail` payload decoded at the exact operand widths
  `Vm::drive` reads (typed constants with raw IEEE-754 bits, pool state,
  cast kind/target, branch targets, call shapes; a payload that runs past
  the code's end records `StepDetail::Truncated`, never a panic).
  `render_trace` renders a captured run into the disassembler's
  deterministic diagnostic style (a golden test pins the two-function
  call-shape listing end to end). The no-sink path costs one `None` check
  per dispatch — the pre-seam behavior. A dispatch overhead bench
  (`benches/vm_trace`: untraced vs traced over a countdown loop, plus the
  captured-steps render) and a consumer walkthrough
  (`examples/execution_trace_walkthrough`) land with it.
- **Fuel budget for the VM (`Vm::with_fuel` → `VmError::FuelExhausted`)**:
  the embedded-execution story's runtime complement of `decode_module`'s
  structural validation — the image seam can refuse a corrupt artifact, but
  nothing stopped a *well-formed* one from looping forever inside the
  embedding host. The machine now refuses a dispatch once the budget is
  spent, returning the explicit error with the executed count (a refused
  dispatch never reaches the trace hook, so the trace ends at the last
  executed step); `Vm::executed` reports the lifetime dispatch count either
  way. Tests pin exact-budget completion, one-short exhaustion, zero fuel,
  and the self-jump `loop` that previously hung stopping at exactly its
  budget; the bench measures the counter's overhead (untraced vs fueled is
  within noise), and the walkthrough demonstrates the stopped loop.
- **Execution-profile seam (`glyim_bytecode_vm::profile`)**: the aggregate
  half of the dynamic observability story — the trace answers what a
  decoded module *did* step by step, the profile answers where its
  dispatches *went* without retaining the transcript: `ProfileCollector`
  impls `TraceSink` and folds the per-dispatch stream into per-opcode and
  per-function dispatch counts plus peak frame/stack depths in
  O(#distinct opcodes + #functions) memory (bounded, never run-length),
  and `render_profile` renders the aggregate deterministically in the
  disassembler's diagnostic style (a golden test pins the two-function
  call-shape profile end to end). The fold is honest at the edges: an
  unknown opcode byte lands in its own displayed `unknown(0xNN)` bucket,
  the faulting dispatch of a failing run is folded in, a function the run
  never entered renders a `0` line, and a fuel-refused dispatch never
  reaches the hook so the profile's total equals `Vm::executed` exactly.
  `hottest(n)` ranks opcodes by dispatch count with raw-byte tie-breaks,
  and `from_steps` re-folds a captured `TraceCollector` transcript into
  the identical profile post-hoc. A dispatch-overhead bench
  (`benches/vm_profile`: unprofiled vs profiled over a countdown loop,
  plus the `from_steps` fold and the render) and a consumer walkthrough
  (`examples/execution_profile_walkthrough`) land with it.
- **`VmError` renders**: the VM's error enum gains `Display` +
  `std::error::Error` (its `ImageError` sibling had both; `VmError` was
  `Debug`-only, so every consumer — the differential harness, the examples —
  printed the derived debug form instead of a diagnostic sentence).
- **Poison-recovery in `glyim-db`'s `TyCtx` locks**: `set_ty_ctx`/`get_ty_ctx`
  recovered from a poisoned `std::sync::RwLock` by panicking; the guarded
  payload is a plain `Option` assignment no panic can leave half-updated, so
  both faces now recover through `PoisonError::into_inner` instead of
  poisoning the caller.
- **README example inventory**: the "Library Examples" list now names every
  example on disk (22 were shipped but unlisted — the bytecode-vm image /
  float / disasm walkthroughs, the codegen assembly walkthrough, the diag /
  frontend / lang / opt / metrics / bench consumers, and 7 ledger faces),
  plus the new execution-trace walkthrough.
- **Module-image disassembler (`render_module` / `render_function`)** in
  `glyim-bytecode-vm`: the observability half of the loadable-image seam —
  once an artifact is decoded, the listing answers "what instructions does
  this carry?" without re-deriving the wire format by hand (the debugging
  counterpart of the assembler, the way `llvm-dis` backs `llvm-as`). The
  rendering is deterministic (the same module renders byte-identical text,
  pinned by a golden test), total over corrupt code (unknown opcode bytes
  list as `unknown(0xNN)` and consume nothing, a payload that runs past
  the code's end lists `<truncated>` and ends the function's listing —
  never a panic, never a silent skip), and read-only. Operand decoding
  mirrors `Vm::drive`'s widths exactly: typed float constants list both
  the IEEE-754 value and the raw bit pattern, `OP_CAST` lists the cast
  kind and target names, `OP_LOAD_CONST_STR` lists the pool index and the
  pool content (an out-of-range index says so), `OP_CALL` lists argc /
  dest / resume target, and block-index mode labels the listing with the
  function's `bN@off:` table so jump targets read against it. A
  disassembly timing bench (`benches/module_disasm`) and a consumer
  walkthrough (`examples/module_disasm_walkthrough`) land with it.
- **`.await` inside an unbounded `loop` body** (the GAP-0006 `loop`-shape
  closure): the async state-machine desugar now normalizes `loop { .. }`
  into the `while true { .. }` shape it already re-drives — a fresh
  literal-`true` condition, the loop body carried over untouched (no
  pattern rename, no increment step) — so the `Start`/`S0`/`Done` machine
  carries the in-loop locals across `Poll::Pending` and `poll()` resumes
  mid-iteration, exactly like the `while` and `for`-over-range shapes. A
  `loop` written as the body's last expression (the parser lowers it into
  the root block's *tail* slot, where the statement scan never looked) is
  spliced into the statement list first, which also unlocks tail-position
  `while`/`for` awaits. The depth-0 `break [v]` inside the loop body is
  the machine's `Done { result }` exit, so `break acc` resolves the future
  with the value the loop exited on. The strict-xfail witness retired into
  a run-pass fixture (`tests/run-pass/behavior/async_loop_await.g`, exit
  code 11); GAP-0006 is re-registered for the remaining shapes (`for` over
  a non-range iterable, destructuring loop patterns) with a rewritten
  compile-fail witness, and the desugar's T0060 diagnostic now names only
  the still-unsupported shapes. A desugar bench shape
  (`benches/async_loop_desugar`), a consumer walkthrough
  (`examples/async_loop_desugar`), and a pipeline runtime differential
  (`glyim-pipeline` `async_loop_runtime`: structure + `block_on` value
  proofs for `break v` / plain `break` / `continue` / guarded awaits) land
  with it.
- **Loadable bytecode module images (`glyim.module-image/1`)**: the
  bytecode VM gains a versioned, self-describing container for whole
  modules — `encode_module` serializes a `Module` (function table, entry
  index, per-function locals/args/block-offset tables and code) into
  deterministic bytes; `decode_module` validates untrusted bytes back
  into an executable module, refusing every structural defect explicitly
  (`ImageError::BadMagic` / `UnsupportedVersion` / `Truncated` /
  `EntryOutOfBounds` / `BlockOffsetOutOfRange` / `TrailingBytes`) and
  logging rejections through `tracing`. `Function`/`Module` grew
  structural equality so round trips assert byte-for-byte identity; a
  decode/encode timing bench (`benches/module_image`) and a consumer
  walkthrough (`examples/module_image_walkthrough`) land with it. The
  emitter's raw `generate` stream is unchanged — this is the missing
  persistence boundary the backend's "testing and embedded execution"
  story needed.
- **Multi-function module-image assembly
  (`BytecodeBackend::generate_module_image`)**: the emitter can now
  produce the loadable artifact — it emits every body, resolves the
  fn-table index space every `OP_CALL` pushes, records per-function
  block-offset tables, appends unreferenced bodies, and picks the body
  named `main` as the entry. Assembly refuses (explicit diagnostics,
  never a broken artifact) opt levels above `O0` (block tables are
  recorded before the peephole pass), empty body sets, and fn tables
  holding several substitution slots for one body (a generic callee
  called at several instantiations). The `vm_interp_diff` differential
  now covers cross-function `OP_CALL`, two-level call chains, and mutual
  recursion end to end (assemble → decode → run vs the MIR interpreter) —
  and it caught a real pre-existing mismatch: the VM bound call
  arguments at `locals[0..]` while the emitter's MIR ABI keeps local 0 as
  the return place and args at `locals[1..=arg_count]`; the VM now
  follows the producer's ABI. An assembly timing bench
  (`benches/module_assembly`) and a walkthrough
  (`examples/module_assembly_walkthrough`) land with it; the dead
  `Chunk::entry` field (written by nothing) is removed.
- **Float arithmetic and casts in the bytecode VM**: the bytecode backend
  emits float constants and float cast kinds that the VM could not execute
  — float constants travelled under the integer `OP_LOAD_CONST` (so float
  arithmetic ran as *integer* arithmetic over the raw `f64` bit patterns)
  and `OP_CAST` was a documented no-op (so `f as i32` silently kept the
  float). The VM grows a `Value::Float` kind (bit-pattern equality, the
  `InterpValue::Float` convention), a typed `OP_LOAD_CONST_F64` constant
  opcode the emitter emits for `MirConstKind::FloatBits`, IEEE-754
  semantics for float binary/unary ops (`1.0 / 0.0` is `+inf`, matching
  LLVM's `fdiv` and the interpreter's T106 convention; comparisons produce
  the VM's `Int` bool encoding), and the interpreter's MIR-14 cast
  semantics: `OP_CAST` now carries a second operand byte naming the target
  primitive (`glyim_bytecode_vm::CastTarget`) so integer truncation is
  width-aware (`(-1i32) as u8 == 255`, not an identity). Every operand-kind
  mismatch is an explicit typed error (`VmError::BinOpTypeMismatch` /
  `UnOpTypeMismatch` / `CastOperandMismatch` / `UnknownCastKind` /
  `UnknownCastTarget` / `ExpectedInt`) — the pre-seam silent reinterpretations
  are gone, and the now-redundant `const_is_int` peephole side-record is
  removed with them. The `vm_interp_diff` differential pins float results
  bit-for-bit against the MIR interpreter (arithmetic, `inf`, negation,
  comparisons feeding branches, `f as i32`, `i32 as u8`, and a
  cross-function float call through the assembled image); a float-execution
  bench (`benches/float_exec`) and a consumer walkthrough
  (`examples/float_casts_walkthrough`) land with it.
- **String constants across the module-image boundary**: the emitter's
  string table was process-local — a string constant was an opaque
  interner index under the integer `OP_LOAD_CONST`, equality-by-index only
  worked while the pool lived in the emitting process, and the payload was
  unrecoverable from the artifact. String constants now travel under their
  own opcode (`OP_LOAD_CONST_STR`, a pool reference), the pool rides inside
  the module image (format version 2: a string-count + length-prefixed
  UTF-8 section after the function table; version-1 images still decode
  with an empty pool), the VM resolves indices into `Module::strings`
  (`Value::Str`, out-of-range indices are an explicit
  `VmError::StringIndexOutOfBounds`), and `OP_EQ`/`OP_NE` over two strings
  compare *content* — no more equality-by-interning-accident. The
  `vm_interp_diff` differential pins string equality through the
  assembled-image path against the interpreter, the module-image bench
  measures the pool section, and the image walkthrough demonstrates it.
- **`.await` inside a `for` loop over a range** (GAP-0001 closed): the
  async state-machine desugar now normalizes `for i in a..b` into the
  `while` shape it already re-drives — a synthetic `let mut __for_i`
  iterator binding hoisted before the loop, the loop variable renamed to
  it inside the body, and an explicit increment step after it — so the
  `Start`/`S0`/`Done` machine carries the iterator and the loop's live
  locals across `Poll::Pending` and `poll()` resumes mid-iteration
  (`0..=` normalizes to a `<=` loop step). The strict-xfail witness
  retired into a run-pass fixture
  (`tests/run-pass/behavior/async_for_loop_await.g`, exit code 3), the
  remaining loop shapes re-registered as the witnessed GAP-0006 with a new
  compile-fail witness, and the desugar's T0060 diagnostic now names the
  still-unsupported shapes (`loop` bodies, `for` over a non-range
  iterable, destructuring loop patterns).
- **LSP workspace folders (`workspace.workspaceFolders` +
  `workspace/didChangeWorkspaceFolders`)**: the session records the
  folder set it serves — seeded from the client's initialize folder list
  (the deprecated `rootUri` as the fallback) and re-derived on change
  events (known removals drop, unknown removals skip, re-adds keep their
  first recording) — and answers which recorded folders contain a path,
  the scoping primitive per-folder work slots into. Folders are session
  metadata keyed by the spelled path; a folder whose URI does not resolve
  to a file path is skipped, never invented.
- **LSP document color (`textDocument/documentColor` +
  `textDocument/colorPresentation`)**: every string literal that spells a
  hex color payload (`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`) answers as
  the swatch site an editor renders inline (the payload extent, quotes
  excluded), and the picker's recolor echo answers the canonical lowercase
  hex spelling over the reported extent — one notation, one presentation.
  The capability walks the stored parse trees, so it works on broken
  code; a presentation over a range no site occupies is never offered.
- **LSP document links (`textDocument/documentLink`)**: the string
  literals of the file-taking builtin macros (`include!`, `include_str!`,
  `include_bytes!`) become clickable file links, resolved against the
  containing document's directory (absolute literals as-is). Non-file
  macros and non-literal arguments answer no link; the capability walks
  the stored parse trees, so it works on broken code.
- **LSP linked editing ranges (`textDocument/linkedEditingRange`)**:
  same-scope occurrences of the cursor's symbol answer as one edit group
  for multi-cursor editing, composing the document-highlight occurrence
  seam (the reference graph's T129 scope-owner filter included).

### Fixed
- **`glyim-layout` fails a niche index overflow instead of writing zero**:
  encoding a niche-encoding enum's tag field / untagged variant into the
  `u32` index space used `u32::try_from(..).unwrap_or(0)`, so an overflowing
  index silently substituted `0` — wiring every `Field(0)` read of the enum
  to the niche slot. A failed conversion now hard-errors
  (`LayoutError::SizeOverflow`) like every other layout overflow.
- Site getting-started tutorial (en/fr) now states the real MSRV (1.95,
  matching `Cargo.toml` and the CI `msrv` job) instead of the stale
  1.94/1.85 pair.
- README + site language reference (en/fr) now list every registered
  builtin macro (`format!`, `vec!`, `matches!`, the `assert!` family,
  `write!`/`writeln!`) and note the stdlib's declarative
  `println!`/`print!`/`eprintln!`/`eprint!`/`panic!`.
- Site crate reference (en/fr) now covers the `tools/` harnesses
  (glyim-ledger, glyim-bench, glyim-metrics, glyim-pilot, ledger-schema).
- `glyim-lsp` no longer silently swallows rejected analysis sends and
  publish-diagnostics notifications: both surface a `tracing::warn`
  naming the affected file.

## [0.10.0] - 2026-11-12

### Added
- **`inventory ignore` / `inventory unignore` (`glyim-ledger inventory
  ignore [ROOT] --item <KEY> --reason <TEXT> [--format text|json]` /
  `glyim-ledger inventory unignore [ROOT] --item <KEY>
  [--format text|json])**: the §3.5 exemption-lifecycle write faces —
  `ledger/inventory-ignore.toml` was the last hand-edited registry the
  §3.5 gate reads: the check fails with `LEDGER-INVENTORY-UNMAPPED` when
  an extracted item is neither claimed nor ignored, but until these faces
  filing the demanded exemption (or retiring one once the item gains a
  claiming feature) meant hand-editing the curated TOML file.
  `inventory ignore` files one `[[ignore]]` block through the new
  `IgnoreSink` seam (`FileIgnoreSink` / `InMemoryIgnoreSink`): the key
  must be produced by an extractor *right now* (exact keys, never globs —
  a pre-registered phantom exemption would silently swallow future
  drift), must not be claimed by a feature (claimed-and-ignored is the
  `LEDGER-INVENTORY-CONFLICT` failure), and must not already be ignored;
  `inventory unignore` removes exactly the one matching block through the
  new `IgnoreRewriter` seam (`FileIgnoreRewriter` /
  `InMemoryIgnoreRewriter`), rewriting the remaining entries canonically
  — the item is unmapped again the moment the rewrite lands, and the
  §3.5 gate is the loud follow-up that forces a feature claim. Both
  faces refuse a tree that does not load cleanly and never leave a
  partial write behind, emit the `glyim.ledger-inventory-ignore/1` /
  `glyim.ledger-inventory-unignore/1` JSON contracts with `--format
  json`, and carry typed `LedgerCliError` variants with `help()` remedy
  lines (`InventoryIgnoreBadItem`, `InventoryIgnoreMissingField`,
  `InventoryIgnoreLoad`, `InventoryIgnoreUnknownItem`,
  `InventoryIgnoreClaimed`, `InventoryIgnoreDuplicate`,
  `InventoryIgnoreWrite`, `InventoryUnignoreBadItem`,
  `InventoryUnignoreLoad`, `InventoryUnignoreMissingField`,
  `InventoryUnignoreUnknownItem`, `InventoryUnignoreWrite`); the
  consumer walkthrough is `cargo run -p glyim-ledger --example
  inventory_ignore_lifecycle` and the bench is `cargo bench -p
  glyim-ledger --bench inventory_ignore`.
- **`gaps unwitness` (`glyim-ledger gaps unwitness [ROOT] --id <GAP-ID>
  [--format text|json])**: the remedy half of the gap trust loop —
  detaches one witnessed gap's witness, restoring honest witness debt.
  The audit face's `missing` verdict is the finding (the registry claims
  a fixture the tree no longer carries); until this face the remedy did
  not exist as a sanctioned command — the witness face refuses an
  already-witnessed gap, so fixing a stale or wrong witness path (a
  fixture moved, renamed, or deleted after `gaps witness` attached it)
  meant hand-editing the curated TOML block. The new
  `GapWitnessDetacher` seam has two implementations —
  `FileGapWitnessDetacher` (reads, edits the matched block, replaces the
  registry file) and `InMemoryGapWitnessDetacher` (tests and in-process
  consumers, with the detach edits recorded for the audit). The detachers
  are deliberately *surgical* like the witness face they invert: only the
  matched `[[gap]]` block's `status` value flips back to `unwitnessed`
  and the aligned `witness` line is removed (the detached path retires to
  the git history of the registry, the retire-face contract), so the
  curated header prose survives byte-for-byte — `gaps witness` followed
  by `gaps unwitness` round-trips the registry bytes. `run_gaps_unwitness`
  validates the request — a usable ID, a registry that loads with zero
  error-severity diagnostics (the witness-face load rule), and a gap
  that actually carries a witness (an unwitnessed gap has nothing to
  detach; the remedy never mints a silent no-op) — and never leaves a
  partial write behind. The dispatch layer refreshes the generated
  `KNOWN_GAPS.md` in the same run through the `render-gaps` pipeline, so
  the `--check` drift gate stays green. 6 new typed `LedgerCliError`
  variants (`GapsUnwitnessMissingField`, `GapsUnwitnessBadId`,
  `GapsUnwitnessUnknownId`, `GapsUnwitnessNotWitnessed`,
  `GapsUnwitnessRegistry`, `GapsUnwitnessWrite`) plus the shared
  `GapsMissingValue`, all carrying their `code()`/`help()` lines. Ships
  the `gaps_unwitness` walkthrough, the `gaps` bench stage, and
  README/lib.rs/main.rs/usage docs.
- **`gaps audit` (`glyim-ledger gaps audit [ROOT] [--format text|json])**:
  the trust-loop closure over the gap-lifecycle write faces — resolves
  every registered gap's witness fixture against the tree, one row per
  gap sorted by ID. The `gaps witness` face checks the fixture at attach
  time, but nothing re-verified the registry against the tree
  afterwards: a fixture that is moved, renamed, or deleted left the
  registry claiming a witness that no longer exists — exactly the silent
  debt plan §3.3 forbids, invisible to every existing face (`check`
  validates registry consistency, the `gaps` report prints the recorded
  path without probing the tree). The new `audit_gaps` verdict pass
  classifies each gap — `witnessed` (the fixture resolves to a readable
  regular file under ROOT), `debt` (honest unwitnessed debt — displayed,
  never failed, the gaps-report rule), `missing` (a registry-claimed
  witness the tree no longer carries, including the bare
  `status = "witnessed"` block that carries no `witness` line at all —
  the serde default makes that a silent claim), and `unreadable` (the
  witness path resolves to something that is not a readable regular
  file). `run_gaps_audit` loads the registry once, reports the same
  sorted diagnostics the `gaps` report face would, and `is_ok()` fails
  the gate on any `missing`/`unreadable` row or error-severity
  diagnostic — the same finding gate `evidence audit` runs over the
  evidence store, applied to the witness paths the write faces record.
  The `glyim.ledger-gaps-audit/1` JSON contract carries the counts and
  the full row list; the text face prints the summary line, every
  non-passing row with its detail (the vanished path), the witness-debt
  reminder, and the diagnostics. Ships the `gaps_audit` walkthrough, the
  `gaps` bench stage, and README/lib.rs/main.rs/usage docs.
- **`evidence retract` (`glyim-ledger evidence retract [ROOT] --fixture
  <PATH> [--out PATH] [--format text|json])**: the evidence-store
  curation removal face — removes every committed record whose
  `fixture_sha256` matches the recomputed digest of one fixture. The
  audit face's `Drift` verdict is the finding (a record that predates a
  fixture edit no longer describes the tree); until this face the remedy
  was hand-editing the canonical JSONL store. The engine reuses two
  seams — `FixtureFingerprint` (the digest is recomputed from the real
  fixture bytes, the same spelling the audit face compares) and
  `EvidenceStore` (the rewrite persists the canonical remainder, so the
  surviving records stay byte-deterministic). `run_evidence_retract`
  validates the request — a relative, `..`-free, backslash-free fixture
  path naming an existing file under ROOT (the fingerprint needs the
  fixture's bytes; a phantom fixture would silently retract nothing),
  a store that loads with zero error-severity diagnostics (a corrupt
  line would be silently destroyed by the rewrite otherwise — the
  waiver-face load rule), and at least one matching record (a fixture
  with nothing committed is a rejection naming the digest, not a silent
  no-op). Nothing is written when validation fails; the persist failure
  reuses the record face's typed `EvidenceWrite` seam. The dispatch
  layer routes every rejection through the typed error seam. 6 new
  typed `LedgerCliError` variants (`EvidenceRetractMissingField`,
  `EvidenceRetractBadFixture`, `EvidenceRetractMissingFixture`,
  `EvidenceRetractFingerprint`, `EvidenceRetractLoad`,
  `EvidenceRetractUnknownFixture`), all carrying their
  `code()`/`help()` lines. Ships the `evidence_retract` walkthrough,
  the `evidence` bench stage, and README/lib.rs/main.rs/usage docs.
- **`gaps open` (`glyim-ledger gaps open [ROOT] --id <GAP-NNNN>
  --class <a|b|c> --title <TEXT> --affects <ID[,ID...]> --caps <L0..L5>
  --opened-in <TAG> [--file ledger/gaps/.../FILE.toml] [--format
  text|json])**: the opening stage of the gap-lifecycle write face —
  files a NEW gap entry into the registry. Until this face the witness
  and retire stages had sanctioned commands, but opening a gap meant
  hand-writing the curated TOML block and remembering the aligned
  registry style, the explicit `status = "unwitnessed"` line, and the
  regenerate-`KNOWN_GAPS.md` step. The new `GapOpener` seam has two
  implementations — `FileGapOpener` (reads, appends, replaces the
  registry file; a missing file is an empty document, so the default
  numbered target is minted on first open) and `InMemoryGapOpener`
  (tests and in-process consumers, with the appends recorded for the
  audit). The engine renders the canonical block in the committed
  registry style (keys padded to the `opened_in` column) and appends it
  after one blank separator, so the curated header prose and every
  sibling block survive byte-for-byte; the block always carries the
  explicit `status = "unwitnessed"` line because the serde default for a
  missing `status` key is `witnessed` — an omitted line would mint
  silent debt, which plan §3.3 forbids. `run_gaps_open` validates the
  request against the loaded registry — the canonical `GAP-NNNN` shape
  (the face mints IDs, so four digits is enforced, not just matched), a
  usable one-line title and `opened_in`, every `--affects` feature
  present in the registry (a gap that limits nothing real is silent
  debt), the ID free (an open gap is never silently shadowed), and a
  relative, existing `.toml` path under `ledger/gaps/` when `--file`
  names one; without `--file` the next numbered
  `ledger/gaps/known/NNNN-gaps-open.toml` is minted (a pure function of
  the tree). A registry that does not load cleanly is never extended.
  The dispatch layer refreshes the generated `KNOWN_GAPS.md` in the same
  run through the `render-gaps` pipeline, so the `--check` drift gate
  stays green. 11 new typed `LedgerCliError` variants
  (`GapsOpenMissingField`, `GapsOpenBadId`, `GapsOpenBadClass`,
  `GapsOpenBadCaps`, `GapsOpenBadTitle`, `GapsOpenBadOpenedIn`,
  `GapsOpenBadFile`, `GapsOpenAffectsUnknown`, `GapsOpenDuplicateId`,
  `GapsOpenRegistry`, `GapsOpenWrite`), all carrying their
  `code()`/`help()` lines. Ships the `gaps_open` walkthrough, the `gaps`
  bench stage, and README/lib.rs/main.rs/usage docs.
- **`gaps witness` (`glyim-ledger gaps witness [ROOT] --id <GAP-ID>
  --witness <PATH>`)**: the gap-lifecycle write face — attaches the
  witness fixture that reproduces one unwitnessed gap, closing the
  witness debt the plan §3.3 report prints and the `next` queue ranks.
  The new `GapWitnessRewriter` seam has two implementations —
  `FileGapWitnessRewriter` (edits the registry file exactly where the
  loaded registry says the block lives) and `InMemoryGapWitnessRewriter`
  (tests and in-process consumers, with the edits recorded for the
  audit). The rewriters are deliberately *surgical* rather than
  canonical: only the matched `[[gap]]` block's `status` value flips and
  the aligned `witness` line is appended, so the curated header prose of
  the committed seed files survives byte-for-byte (the opposite tradeoff
  to the waiver faces, whose machine-written document is canonically
  re-serialized). `run_gaps_witness` validates the request against the
  registry and the tree — a relative, `..`-free, backslash-free path
  naming an existing fixture (a phantom witness would trade visible debt
  for silent debt) — refuses a registry that does not load cleanly, and
  never silently re-witnesses an already-witnessed gap. The dispatch
  layer refreshes the generated `KNOWN_GAPS.md` in the same run through
  the `render-gaps` pipeline, so the `--check` drift gate stays green.
  8 new typed `LedgerCliError` variants (`GapsWitnessMissingField`,
  `GapsWitnessBadId`, `GapsWitnessBadWitness`, `GapsWitnessUnknownId`,
  `GapsWitnessRegistry`, `GapsWitnessAlreadyWitnessed`,
  `GapsWitnessMissingFixture`, `GapsWitnessWrite`, plus the shared
  `GapsBadFormat`), all carrying their `code()`/`help()` lines. Ships
  the `gaps_witness` walkthrough, the `gaps` bench stage, and
  README/lib.rs/main.rs/usage docs.
- **`gaps retire` (`glyim-ledger gaps retire [ROOT] --id <GAP-ID>`)**:
  the closure half of the gap lifecycle — removes one gap's `[[gap]]`
  block from its registry file entirely. The rendered `KNOWN_GAPS.md`
  states that closed entries retire to the git history of the file and
  of the registry; until this face the removal itself had no sanctioned
  command. The new `GapRetirer` seam has two implementations —
  `FileGapRetirer` (drops exactly the matched block's lines, swallowing
  the block's trailing separator when another block follows) and
  `InMemoryGapRetirer` (tests and in-process consumers, with the
  removals recorded for the audit). `run_gaps_retire` validates the ID,
  refuses a registry that does not load cleanly (the revoke-face load
  rule), and reports the remaining open-gap count through the
  `glyim.ledger-gaps-retire/1` JSON contract and the text face. The
  dispatch layer refreshes the generated `KNOWN_GAPS.md` in the same
  run. 5 new typed `LedgerCliError` variants
  (`GapsRetireMissingField`/`GapsRetireBadId`/`GapsRetireUnknownId`/
  `GapsRetireLoad`/`GapsRetireWrite`), all carrying their
  `code()`/`help()` lines. Ships the `gaps_retire` walkthrough, the
  retire bench stage, and README/lib.rs/main.rs/usage docs.
- **French documentation (i18n)**: the `glyim-docs` site ships a
  complete French translation of 19 content pages under
  `docs/src/content/docs/fr/`.
- **`waivers prune` (`glyim-ledger waivers prune`)**: retires every
  expired C9 exception in one call — the batch counterpart of `waivers
  revoke` (one exact ID at a time) for the moment an operator clears the
  whole expired-inventory debt. The new `WaiverPruner` seam (the
  many-expired counterpart of `WaiverRewriter`) has two implementations —
  `FileWaiverPruner` (parses the committed document, partitions the
  entries by expiry, re-serializes the remaining entries canonically,
  and replaces the file; a non-clean document is refused, never
  truncated) and `InMemoryWaiverPruner` (tests and in-process consumers,
  with the pruned IDs recorded for the audit). `run_waivers_prune`
  validates the document (zero error-severity load diagnostics), and a
  document with no expired waiver is **never rewritten** — the seam is
  not invoked and the file bytes stay untouched. The outcome rows report
  ID order regardless of the document's construction path (the
  `assess_waivers` contract) through the `glyim.ledger-waivers-prune/1`
  JSON contract and the text face. 2 new typed `LedgerCliError` variants
  (`WaiversPruneLoad`/`WaiversPruneWrite`), both carrying their
  `code()`/`help()` lines. Ships the `waivers_prune` walkthrough, the
  prune bench stage, and README/lib.rs/main.rs/usage docs.
- **`release-check` (`glyim-ledger release-check <VERSION> [ROOT]`)**:
  the plan §10 final release gate — the master plan names the face in
  the CLI table ("`ledger release-check vX.Y.Z` — Final release gate"),
  defines its contract in the release-criteria section ("passes iff all
  24 readiness gates are green on the release commit, evidence is
  reproducible (RG-04/17), and no waiver is active"), and requires it at
  M12.7, but no command composed the three shipped verdict axes into the
  one verdict a release commit needs. The new `ReleaseCheckSource` seam
  (the composition counterpart of `HandoffSource`) has two
  implementations — `FileReleaseCheckSource` (runs the real M1.12 gate
  report, M1.13 determinism self-check, and C9 waiver health over the
  registry, through the same executors those faces use, so a release
  verdict can never disagree with an individual face run) and
  `InMemoryReleaseCheckSource` (tests and in-process consumers) — and
  the `ReleaseCheckRenderer` seam (the `WaiversRenderer` pattern) renders
  the text report and the `glyim.ledger-release-check/1` JSON contract
  from one `ReleaseCheckOutcome`. The gate is `is_ok`, clause by clause
  the plan sentence: every declared gate green — a `fail` blocks and an
  `unmeasured` verdict blocks too (plan M1.12: displayed, never silently
  passed — the blocking list names them in declaration order), the
  self-check reports no drift, no waiver is active or expired (the C9
  health contract keeps the release gate from ever being greener than
  the faces it composes), and no error-severity diagnostic across the
  composed faces (warnings display, not fail). The `<VERSION>` argument
  is validated at parse time (`vMAJOR.MINOR.PATCH`, `v` optional; no
  pre-release tags or build metadata — a release gate gates a plain
  triple) and the verdict names it in both faces. 4 new typed
  `LedgerCliError` variants (`ReleaseCheck*`), all carrying their
  `code()`/`help()` lines. Ships the `release_check` walkthrough, the
  release bench stage, and README/lib.rs/main.rs/usage docs.
- **`waivers extend` (`glyim-ledger waivers extend`)**: re-pins one C9 exception's expiry to `today + --days` (default 7, horizon 14) — the sanctioned counterpart the `WaiversGrantDuplicate` help line already points to. The new `WaiverExtender` seam (the mutation counterpart of `WaiverRewriter`) has two implementations — `FileWaiverExtender` and `InMemoryWaiverExtender` — and the identity (ID, feature, reason, issue) is never rewritten; only the expiry moves. 6 new typed `LedgerCliError` variants (`WaiversExtend*`), all carrying their `code()`/`help()` lines. Ships the `waivers_extend` walkthrough, the extend bench stage, and README/lib.rs/main.rs/usage docs.
- **`evidence audit` (`glyim-ledger evidence audit`)**: audits the committed store against the filesystem — recomputes every record's `fixture_sha256` from the fixture bytes and reports `ok`/`missing`/`drift`/`unreadable` verdicts; any finding fails the exit code (the trust-loop closure `evidence check` cannot express). The new `FixtureFingerprint` seam has two implementations — `FileFixtureFingerprint` (sha2 over the raw bytes) and `InMemoryFixtureFingerprint` (precomputed digests). Ships the `evidence_audit` walkthrough, the audit bench stage, and README/lib.rs/main.rs/usage docs.

- **C9 waiver-retirement face (`glyim-ledger waivers revoke`)**:
  `waivers grant` is append-only by design — once a waiver's regression
  fix landed (or its horizon expired and the C9 gate started failing
  CI), the only way to retire the entry was hand-editing
  `ledger/waivers.toml`, bypassing the canonical-ID and
  canonical-serialization discipline every other face enforces. The new
  `WaiverRewriter` seam (the rewrite counterpart of `WaiverSink`) has
  two implementations — `FileWaiverRewriter` (parses the committed
  document, drops the one exact-ID match, re-serializes the remaining
  entries canonically, and replaces the file; a document that fails to
  parse is refused, never truncated) and `InMemoryWaiverRewriter`
  (tests and in-process consumers, with the removal recorded for the
  audit). `run_waivers_revoke` validates the request (usable ID; the
  committed document must load with zero error-severity diagnostics —
  a duplicate-ID file fails here; the ID must be present, and an
  unknown ID writes nothing and names a few existing candidates),
  removes exactly one block, and reports the remaining count through
  the `glyim.ledger-waivers-revoke/1` JSON contract and the one-line
  text face. Five new typed `LedgerCliError` variants
  (`WaiversRevokeMissingField`/`WaiversRevokeBadId` for the parser;
  `WaiversRevokeUnknownId`/`WaiversRevokeLoad`/`WaiversRevokeWrite`
  for the write face), all carrying their `code()`/`help()` lines.
- **§3.7 session-handoff face (`glyim-ledger handoff`)**: the master
  plan's CLI table names a `ledger handoff` face ("generates the (short)
  session handoff from state") that did not exist — composing the
  readiness state into one session-opening read meant running four
  commands (`compute`, `next`, `waivers`, `gaps`) and merging them by
  hand. The new `HandoffSource` seam (the composition counterpart of
  `WaiverSource`) has two implementations — `FileHandoffSource` (runs
  the real compute, `next`, and waiver-health faces over the registry,
  through the same loaders those faces use, so a handoff can never
  disagree with them) and `InMemoryHandoffSource` (tests and in-process
  consumers) — and the `HandoffRenderer` seam (the `WaiversRenderer`
  pattern) renders the marker-delimited Markdown document face and the
  `glyim.ledger-handoff/1` JSON contract from one `HandoffOutcome`.
  The document is short by contract: the readiness histogram (features,
  evidence records, L0–L5 counts, error diagnostics), the top-N work
  queue (`--items N`, default 5, max 20 — ranks stay honest so a slice
  still shows where items stand in the full queue), waiver health with
  the next expiring waiver, and the witness debt. The gate is
  `HandoffOutcome::is_ok`: zero error-severity diagnostics across the
  three composed faces **and** no expired waiver (constitution C9) —
  the debt is rendered, never silent, and the exit code fails exactly
  as the composed faces would. `--out PATH` writes the rendered
  document (default `HANDOFF.md` / `HANDOFF.json` by face) and
  `--check` byte-compares the committed document against the composed
  state (the M1.10 drift-gate contract, mirrored from the `status`
  face; a committed `HANDOFF.md` is not yet wired into CI — wire the
  job once the face settles). Five new typed `LedgerCliError` variants
  (`HandoffBadFormat`/`HandoffMissingValue`/`HandoffBadItems` for the
  parser; `HandoffDrift`/`HandoffWrite` for the write/check face), all
  carrying their `code()`/`help()` lines. The engine sorts the queue
  by rank before slicing (input order never leaks into the render) and
  pins `today` on the waivers outcome (no second clock consult).
- **C9 waiver-health face (`glyim-ledger waivers`) + curated grant write
  face (`glyim-ledger waivers grant`)**: `ledger/waivers.toml` was read
  only inside the `diff` regression gate — an expired or horizon-violating
  waiver stayed invisible until a regression actually needed covering,
  and there was no way to inspect the live exception inventory at all,
  so the C9 sentence "expired waivers fail CI" was not directly
  checkable. The new `WaiverSource` seam (the load counterpart of
  `SnapshotStore`/`EvidenceSource`) has two implementations —
  `FileWaiverSource` (reads `<ROOT>/ledger/waivers.toml` through the same
  `load_waivers` loader and diagnostic codes the `diff` gate uses, so the
  two faces can never disagree) and `InMemoryWaiverSource` (tests and
  in-process consumers) — and the `WaiversRenderer` seam (the
  `KnownGapsRenderer` pattern) renders the text report and the
  `glyim.ledger-waivers/1` JSON contract from one `WaiversOutcome`. The
  gate is `WaiversOutcome::is_ok`: no error-severity load diagnostic
  (schema, duplicate ID, horizon violation) **and** no row whose verdict
  is `Expired`; a waiver inside its horizon is report content (the
  remaining-days column is the operator's reminder), never a failure.
  `waivers grant` is the write counterpart: it validates the request
  against the registry (`--feature` must name a declared feature,
  `--issue` must be an `https://` link, `--reason` must be usable,
  `--days` must stay inside `WAIVER_MAX_AGE_DAYS`, CLI default 7),
  composes the deterministic `WVR-<YYYYMMDD>-<feature slug>` ID, and
  appends the canonical `[[waiver]]` block through the `WaiverSink` seam
  (`FileWaiverSink` appends to the committed document with
  newline-normalization, `InMemoryWaiverSink` records the fragment for
  tests; the `toml` crate escapes, never hand-quoting). A same-day
  re-grant is a duplicate error, never a silent overwrite, and nothing is
  written when validation fails. Eight new typed `LedgerCliError`
  variants (`WaiversBadFormat`/`WaiversMissingValue` for the health face;
  `WaiversGrantMissingField`/`WaiversGrantBadDays`/`WaiversGrantBadIssue`/
  `WaiversGrantUnknownFeature`/`WaiversGrantRegistry`/`WaiversGrantDuplicate`/
  `WaiversGrantWrite` for the grant face), each with its `help:` remedy
  line. Ships the `waiver_health` and `waivers_grant` walkthroughs (the
  grant walkthrough writes into a throwaway registry, never the live
  one), the `waivers` bench (assess + both faces + the grant validation
  path over a 500-waiver set), and README/lib.rs/main.rs/usage docs.

### Added
- **Goto declaration (`textDocument/declaration`)**: the missing member
  of the goto family — goto-definition answers the *definition* (the
  item, wherever it lives), but nothing answered "where is this name
  declared *for this file*". The new `goto_declaration` engine resolves
  the cursor's identifier through a `DeclarationSource` seam: a `use`
  statement binding the name declares it (the import wins over a
  same-named local item — that is what the user wrote to bring the name
  into scope), a locally-declared item declares itself, and a name that
  is neither imported nor declared in the file answers `null` honestly
  instead of masquerading as a second definition request. The production
  `ParseTreeDeclarationSource` walks the parse trees the analysis driver
  already stores (`db.syntaxes`) — glob (`use foo::*`) and `_` aliases
  bind no single name and are skipped — so no re-analysis happens on the
  request path and broken files still answer. Ships the policy tests
  over a mock source, the database-backed request tests, the
  `goto_declaration` bench, and the `goto_declaration_walkthrough`
  example.
- **Server log file (`glyim-lsp --log-file X`)**: the flag was parsed
  and documented ("tee tracing to X") but `run_server` accepted it as
  `_log_file` and never used it — every tracing event the engines emit
  (`debug!` per request, `warn!` per dropped edit) went nowhere. The new
  `server_log` module closes the seam: `FileLog` opens the target file
  and implements `tracing_subscriber`'s `MakeWriter` (one `Mutex`
  writer, line-buffered), `install_global` wires it as the global
  default subscriber emitting JSON lines, and `run_server` installs it
  when the flag is present — a file that cannot be opened degrades to a
  one-line stderr note and a server that still runs, because losing the
  log must never cost the session. Ships the file-log tests (bytes land,
  events round-trip as JSON) and the `server_log_walkthrough` example.
- **Inlay hints (`textDocument/inlayHint`)**: inline `: Ty` annotations
  on un-annotated `let` bindings, resolved from the Tier 6.4 HIR +
  typeck state through a `BindingTypeSource` seam (`HirBindingTypeSource`
  production impl), with an honest skip list (annotated bindings, types
  with no stable user-facing spelling, bindings outside the requested
  range). Routed, advertised, conformance-pinned, tested over a mock and
  over the database, benched (`inlay_hint`), and walked through in
  `inlay_hint_walkthrough`.
- **Call hierarchy (`textDocument/prepareCallHierarchy` + incoming and
  outgoing calls) and signature help (`textDocument/signatureHelp`)**:
  the call-hierarchy requests answer the function item under the cursor
  and walk the reference graph's call records for callers and callees;
  signature help resolves the innermost recorded call containing the
  cursor and renders the callee's signature with the active parameter
  counted from the source text. Both read the same reference graph and
  symbol index the other request engines use (seams `CallGraphSource` /
  `CallSiteSource` / `SignatureSource`), are routed, advertised,
  conformance-pinned, tested over mocks and the database, benched, and
  walked through in dedicated examples.

### Fixed
- `glyip` no longer silently swallows a corrupt `Glyip.lock`:
  `resolve_lockfile` read the lockfile with
  `Lockfile::read_from_dir(...).unwrap_or_default()`, which conflated
  "no lockfile yet" (fine — `read_from_dir` already answers an empty one)
  with "a resolution record that cannot be parsed" — the user's pin
  record was discarded without a trace and re-resolved as if it had never
  existed. The read error now propagates, and a regression test pins the
  contract (`crates/glyip/src/tests/lockfile_advanced.rs`).
- `STATUS.md` / `STATUS.json` were committed stale after the ledger
  feature registry gained the newer LSP capability declarations — the
  committed status said 225 features over 9 `TOOL-LSP-*` rows while the
  registry carried 233 over 17; the `ledger-check` CI job's
  `status . --check` byte-compare would fail on every run. Both are
  regenerated from the registry (and the two new capability entries,
  `TOOL-LSP-DECLARATION` and `TOOL-LSP-LOG-FILE`, are declared in
  `ledger/features/tooling/lsp.toml` in the same pass).
- Two latent rustc-1.100 clippy breaks (found by the
  `--all-targets --all-features` gate): `tools/glyim-metrics/examples/
  baseline_check.rs` printed a `{}` placeholder with no argument
  (`no committed baseline at {} — skipped`) — a runtime panic on the
  out-of-repository arm that the newer `format_args` diagnostic rejects
  at compile time; the committed-baseline path is now the argument. And
  `tools/glyim-ledger` built two `cloned_ref_to_slice_refs` sites (the
  `evidence_store` walkthrough and a `merge_records` test) —
  `&[run.clone()]` slice literals became `std::slice::from_ref(&x)`.

- **Evidence-store curation face (plan §3.4 store remainder)**:
  `tools/glyim-ledger/src/evidence.rs` closes the gap between the M1.5
  evidence *emitter* (`glyim-test` records under `GLYIM_TEST_EVIDENCE`)
  and the M1.8 *consumer* (`ledger/evidence/*.jsonl` discovered by
  `ProjectEvidenceSource`): nothing owned the store those records are
  committed into, so a harness run reached the ledger only by
  hand-copying JSONL out of `target/`. The new `EvidenceStore` seam (the
  write counterpart of the read-side `EvidenceSource` trait) has two
  implementations — `FileEvidenceStore` (the committed
  `ledger/evidence/*.jsonl` layout: one canonical
  `glyim.evidence/1` line per record, missing parent directories created,
  an absent store file an honest fresh store) and
  `InMemoryEvidenceStore` (tests and in-process consumers, same
  canonicalization contract). Wired as
  `glyim-ledger evidence record [--out PATH] [--format text|json]
  <SRC...>` — validate harness-emitted records against the registry
  (schema tag, M1.5 record invariants, unknown feature IDs; every
  rejection is an error diagnostic, nothing invalid reaches the store),
  merge them with whole-record dedupe into canonical order
  (`merge_records`, idempotent and order-insensitive so re-recording the
  same run is byte-identical; `glyim.ledger-evidence-record/1` JSON
  face) — and `glyim-ledger evidence check [ROOT]` — validate the
  committed store itself and report its coverage (files, records,
  duplicates, distinct fixtures, features covered, per-outcome
  histogram; `glyim.ledger-evidence-check/1`, error diagnostics fail the
  exit code, the CI gate face). Four new typed `LedgerCliError` variants
  (`EvidenceMissingSources`/`EvidenceMissingValue`/`EvidenceBadFormat`/
  `EvidenceSubcommand`) plus `EvidenceWrite` for the store-persist
  failure, each with its `help:` remedy line. Ships the `evidence_store`
  example (curate → idempotence → check, over a synthetic registry or a
  real root) and the dependency-free `evidence` bench (2000-record merge
  into a 2000-record store, 4000-record check, and the idempotent
  re-record no-op). The committed `ledger/evidence/test.jsonl` is
  machine-observed: generated by running the corpus suites with
  `GLYIM_TEST_EVIDENCE` and curated through the new record face.
- **STATUS drift gate (M1.10 remainder: "STATUS is up to date")**:
  `tools/glyim-ledger/src/status.rs` lands the slice the M1.9 first
  slice deliberately deferred (its module doc named the gate "a later
  slice"): `run_status_out` renders a status artifact and either writes
  it (`--out PATH`, default `STATUS.md` / `STATUS.json` by face under
  ROOT — `STATUS_MD_TARGET` / `STATUS_JSON_TARGET`) or, with `--check`,
  byte-compares the committed document against the pure
  `(registry, evidence, policy)` render and fails with the typed
  `glyim-ledger::status_drift` error (the exact drift-gate contract
  `render-gaps --check` established for `KNOWN_GAPS.md`; write failures
  go through the new `glyim-ledger::status_write`). The committed
  `STATUS.md` / `STATUS.json` are generated from the committed evidence
  store, so the M1.10 CI gate and the M1.8 engine can never disagree
  with what a contributor sees. Wired through `parse_status` (`--out`,
  `--check`), the `status` usage/help text, and the `main.rs` dispatch
  (write/check verdicts print, drift fails like a CLI error); pinned by
  write-face, check-OK, drift, missing-document, explicit-target, and
  CLI-parse tests.

- **Generated KNOWN_GAPS.md (M1.9 remainder)**:
  `tools/glyim-ledger/src/render.rs` closes the drift class the M1.7
  migration left behind — the machine-checked status lives in
  `ledger/gaps/known/*.toml` while `KNOWN_GAPS.md` stayed hand-maintained
  (its own audit note records one stale revision). The document is now a
  pure function of the gap registry: the `KnownGapsRenderer` seam has a
  Markdown face (the committed `KNOWN_GAPS.md` — provenance header,
  witness-debt table, one section per open gap sorted by ID) and a JSON
  face (`glyim.known-gaps/1`, the readiness page's future input), both
  consuming the M1.7 `GapOutcome` verbatim so the file can never disagree
  with `glyim-ledger gaps`. Wired as `glyim-ledger render-gaps [ROOT]
  [--format md|json] [--out PATH] [--check]`: `--out` writes, `--check`
  byte-compares the committed document and fails with the typed
  `glyim-ledger::render_drift` error (plan M1.9 DoD: "outputs
  deterministic (golden)" — pinned by a full golden fixture and
  repeat-render byte-equality tests). `KNOWN_GAPS.md` is regenerated from
  the registry in this change; the closed-entry history retires to git
  (the registry holds only open gaps, per the RG-10 verdict's contract).
  Ships the `render_known_gaps` example and the dependency-free `render`
  bench (both faces over a 500-gap registry).
- **Committed corpora directory (M2.3 remainder)**:
  `tools/glyim-bench/src/corpus_dir.rs` lands the slice the master plan's
  M2.3 exit gate calls "corpora committed with hashes" on top of the
  slice-1 substrate: the `CorpusSink` write seam, `DirCorpusSink`
  (conventional `perf/corpus/` layout — one `<family>-<seed>-<size>.g`
  file per program plus the `glyim.corpus/1` `corpus.json` manifest),
  `CorpusDirPlan` flag parsing (families × seeds × size, default all
  seven families), and `check_corpus_dir` — which loads the manifest,
  runs the substrate's template-verify contract, then re-hashes every
  committed `.g` file, reporting missing/hand-edited programs as gate
  rows (exit non-zero, the same verdict-not-error shape as the diff and
  budget gates). Wired as `glyim-bench corpus write|check [--dir PATH]
  [--families LIST] [--seeds LIST] [--size N]` with four new typed
  `BenchError` variants (`CorpusDirRead`/`CorpusDirWrite`/
  `CorpusFileMissing`/`CorpusFileMismatch`), the `corpus_commit`
  example, and the `corpus_dir` bench (28-program write + check).
- **Release-gate report (M1.12)**: `tools/glyim-ledger/src/gates.rs`
  implements the plan §6 M1.12 exit gate — "Readiness gates RG-01…RG-24
  declared in `ledger/gates.toml`; `unmeasured` allowed but displayed;
  `ledger gates` prints all 24". The declaration face is a new strict
  schema beside the policy loader (`tools/ledger-schema/src/gates.rs`,
  `glyim.ledger-gates-registry/1`, `deny_unknown_fields`): each `[[gate]]`
  row carries the plan's `RG-NN` id, title, criterion, and the evidence
  source the verdict engine may consult today — a closed enum (`none` /
  `levels` / `gaps`) so a declaration can never claim an input the engine
  does not have; verdicts have no on-disk field (constitution C1: status
  is computed, never typed). The engine evaluates every declared gate per
  source — RG-09 over the M1.8 compute outcome (core/std features below
  L4, then core below the unmeasurable-L5, reported as fail naming the
  unmeasured hardening axes), RG-10 over the M1.7 gap registry (open
  class-b gaps, class-a gaps without a pinned diagnostic, witness debt —
  the registry holds only open gaps, so every counted entry is open by
  construction), `none` as the honest `unmeasured` (displayed, never
  silently passed, never failed by itself: the M1.12 exit gate is "prints
  all 24", so the command exits non-zero only on load/validation
  diagnostics; CI blocking stays with the later RG wiring slices).
  Wired as `glyim-ledger gates [ROOT] [--evidence PATH]
  [--format text|json]` over the `glyim.ledger-gates/1` contract, with
  the `release_gates` example and the dependency-free `gates` bench (24
  gates over a 500-feature registry + gaps). `ledger/gates.toml` declares
  all 24 plan gates; on today's tree the report prints 0 pass, 2 fail
  (RG-09, RG-10), 22 unmeasured — the honest release state.
- **Determinism self-check (M1.13 first slice)**:
  `tools/glyim-ledger/src/verify.rs` implements the plan §6 M1.13
  "run `compute` twice → identical bytes" half as a command:
  `verify_determinism` runs every ledger pipeline — compute
  (text/JSON/snapshot document), status (Markdown), and the M1.12 gates
  report (text/JSON) — **twice**, from two independent load passes over
  the same root, and compares the rendered bytes per check (fixed
  `CHECK_NAMES` order; the order is part of the `glyim.ledger-verify/1`
  contract). Unlike the `gates` report, `verify` is a *gate*: a drifted
  check or an error-severity diagnostic fails the command (exit 1), so
  CI can wire it directly. The checker reuses the executors verbatim and
  adds no IO boundary, so future evidence sources participate
  automatically; the two-machine comparison (committed golden + CI job)
  is deliberately left to the later M1.13 slices. Wired as
  `glyim-ledger verify [ROOT] [--evidence PATH] [--format text|json]`,
  with the `verify_determinism` example and the dependency-free `verify`
  bench (6 double-run checks over the 500-feature synthetic registry).
- **Matrix gate with the geo-mean aggregate budget (M2.5 remainder)**:
  `tools/glyim-bench/src/matrix.rs` implements the plan §325 / §6 M2.5
  perf-compare criterion — "fail >2% on a benchmark **or >0.5%
  geo-mean**" — the aggregate clause no existing gate could express: a
  change that slows every benchmark by +1% passes each per-benchmark
  budget and is caught only by the matrix. The `BaselineSource` trait
  (`HistoryBaselineSource` selects the latest recorded run of each
  program from a `HistoryStore`, lazily cached; `InMemoryBaselineSource`
  backs tests) keeps the engine IO-free and lets the baseline layout
  evolve; `diff_matrix` reuses the unchanged M2.5 `diff_reports` engine
  per row (rows in input order, no clock, plan C6), and
  `execute_diff_matrix` wires the `ReportStore` + `BaselineSource`
  seams. The instruction geo-mean is measured only when *every* row
  carried instructions (the budget gate's honesty rule: a geo-mean over
  a subset is not coverage). Wired as `glyim-bench matrix
  --history PATH [--max-delta-pct] [--max-geo-delta-pct] REPORT...`
  over the `glyim.bench-matrix/1` outcome, with the `matrix_gate`
  example and the dependency-free `matrix_engine` bench (8 programs ×
  256 history records). A missing baseline is now a typed
  `BenchError::BaselineUnavailable` with a remedy-naming help line
  (previously the misleading `InvalidPlan` whose help talked about
  `--iterations`).
- **History trend analyzer (M2.5 complement)**:
  `tools/glyim-bench/src/trend.rs` measures the **total drift** across
  the last N recorded runs of one program (`glyim.bench-trend/1`) —
  the accumulated multi-PR regression the latest-baseline diff can
  never see, because every individual PR stayed within its own budget
  while the window ratchets +1% per run. `trend_over_history` is the
  pure engine (append-order window, first-to-last drift, the diff
  engine's degenerate zero-first-mean rule), `execute_trend_from_history`
  wires the `HistoryStore` seam, and the instruction drift is measured
  only when every window point carried instructions. Wired as
  `glyim-bench history trend --history PATH --program P [--window N]
  [--max-drift-pct]`, with the `history_trend` example and the
  `trend_engine` bench (256 records × 2 programs, window 32).
- **Perf-history store (M2.5 first slice)**:
  `tools/glyim-bench/src/history.rs` implements the plan's
  `perf/history/*.jsonl` seam — the committed memory the M2.5 diff gate
  diffs against. The `HistoryStore` trait (`JsonlHistoryStore` appends
  one self-describing `glyim.bench-history/1` record per line and loads
  a missing file as an empty history; `InMemoryHistoryStore` backs
  tests) is path-agnostic (`program_slug` derives the conventional
  per-benchmark filename), `latest_baseline` is the pure selection —
  the last record measuring the program, file order = append order, no
  clock (plan C6) — and `execute_diff_from_history` wires the stores
  into the unchanged M2.5 diff engine (`baseline_path` names the history
  file with the `#latest` selector). Wired as
  `glyim-bench history record|show|diff`, with the `history_baseline`
  example and the dependency-free `history_store` bench.
- **Absolute budget gate (M2.6 first slice)**:
  `tools/glyim-bench/src/budget.rs` implements the plan's budgets seam —
  constitution C7's absolute half ("budgets are tests"): a committed
  `glyim.bench-budgets/1` TOML document (`perf/budgets.toml`, one
  `[[budget]]` row per program with `max_wall_mean_ns` /
  `max_instructions_mean` bounds) is enforced against written
  `glyim.bench/1` reports. Split exactly like the M2.5 diff slice: the
  `BudgetStore` trait (`FileBudgetStore` on disk, `InMemoryBudgetStore`
  for tests) abstracts where the budgets come from, `check_budgets` is
  the pure `(reports, budgets) → verdicts` engine, and `execute_budget`
  wires both stores. Verdicts extend the M2.2 gate's inclusive `<=`
  boundary with two honesty rules: a bound on a metric the report does
  not carry is `Unmeasured` (a wall-only run cannot silently satisfy an
  instruction budget), and a program with no budget row fails outright —
  a benchmark that never declared a budget cannot be "within" one.
  Renderers are the deterministic text table and the
  `glyim.bench-budget/1` JSON contract; wired as
  `glyim-bench budget [--budgets perf/budgets.toml] [--json OUT]
  REPORT...`, with the `budget_check` example and the dependency-free
  `budget_engine` bench.
- **Readiness-regression gate (M1.10 first slice)**:
  `tools/glyim-ledger/src/diff.rs` implements the plan's `ledger diff` —
  a per-feature level diff between two `glyim.ledger-snapshot/1` snapshot
  documents (produced by `glyim-ledger compute --snapshot PATH`) with the
  constitution-C9 waiver gate: a computed-level drop (or a feature
  dropped from the registry) fails the gate unless an unexpired
  `ledger/waivers.toml` waiver covers it, a waiver expiring beyond the
  14-day horizon is inert (error diagnostic), and an *expired* waiver that
  would have covered a regression is itself an error so a stale
  `waivers.toml` can never mask a real regression. Split exactly like
  `compute.rs`: the `SnapshotStore` trait (`FileSnapshotStore` on disk,
  `InMemorySnapshotStore` for tests) abstracts where snapshots come from,
  `diff_levels` is the pure engine, `execute_diff`/`run_diff` wire it;
  hand-rolled proleptic-Gregorian date arithmetic (`NaiveDate`,
  Hinnant's day-number algorithms) keeps the dependency-light convention.
  Renderers are the text report and the `glyim.ledger-diff/1` JSON
  contract; wired as
  `glyim-ledger diff <BASE> <CURRENT> [ROOT] [--format text|json]`, with
  the `readiness_diff` example and the dependency-free `diff` bench.
- **Scaling analyzer (M2.4 first slice)**:
  `tools/glyim-bench/src/scaling.rs` implements the plan's M2.4 —
  measure a `{size}`-parameterized command at every input size and fit
  the log-log scaling exponent. `fit_scaling` is a pure ordinary
  least-squares over `(log size, log mean wall time)` (exact recovery of
  power laws, constant series pinned to exponent 0 by convention),
  `substitute_size` is the argv contract, `run_scaling` drives the M2.2
  runner per size, and `execute_scaling` fits a stored report history
  through the M2.5 `ReportStore` seam — a committed report history
  doubles as a scaling-history input without a new IO boundary. Budget
  semantics mirror the M2.5 gate (exponent strictly above the budget is
  `SUPERLINEAR` and fails); wired as
  `glyim-bench scale [--sizes LIST] [--max-exponent E] -- CMD...`, with
  the `scaling_fit` example and the dependency-free `scaling_engine`
  bench.

### Fixed
- Docs drift, verified against the code: the Astro CLI reference
  (`glyim-docs/src/content/docs/reference/cli.mdx`) listed only
  `obj/exec/mir/llvm-ir/asm/cdylib` emit kinds — `syntax`, `stats`, and
  `run` (documented in the README's flag table) are added, the flag
  table gains the M2.1 `--time-passes` / `--self-profile` measurement
  flags, and `--backend` documents the `interp` backend the driver
  accepts for `--emit run`; the glyip reference's
  `glyip build --release -O2` example used a short flag the binary does
  not have (the parser only takes `--opt-level`, as the README already
  showed) and now reads `--opt-level 2`.
- `CONTRIBUTING.md` cited a hand-maintained "KNOWN_GAPS §3.5.26"
  section that no longer exists (the document is generated from
  `ledger/gaps/known/*.toml` and carries GAP-NNNN entries, not numbered
  sections); the sentence now points at the registry's link-failure
  diagnostics. `SECURITY.md` claimed the gaps file "documents dozens of
  incomplete features" — the generated registry holds exactly 5 open
  gaps; the claim is corrected to the machine-checked count.
- `README.md`'s tooling summary listed the `--emit` kinds without
  `run` (its own flag table two hundred lines below already documented
  it); the list now includes `run`.
- `glyim-docs/src/content.config.ts.bak.20261112-165003` was a tracked
  editor backup file (committed hygiene slip); removed from the index.
- `tools/glyim-ledger/Cargo.toml`: the package description still
  enumerated the M1.7-era command set (`compute` onwards missing); it
  now matches the shipped surface including the evidence faces.
- `tools/glyim-bench/src/runner.rs` / `tools/glyim-bench/src/trend.rs`:
  the last two `expect` calls in non-test code (against the
  CONTRIBUTING no-`unwrap`/`expect` rule) are replaced with typed
  fallbacks — `run_iterated`'s `summarize(...).expect("iterations >= 1")`
  becomes an `ok_or_else` into `BenchError::InvalidPlan` behind the
  existing zero-iteration guard, and `execute_trend_from_history`'s two
  `.expect("covered above")` endpoint reads become an
  `and_then`-destructured pair that degrades to the guard's
  no-instruction-drift branch instead of panicking. Same semantics on
  every reachable path; `glyim-bench` now has zero `expect`/`unwrap`
  outside tests.
- `tools/glyim-bench/src/main.rs`: the `diff` arm passed
  `json.as_ref()` (`Option<&PathBuf>`) into `execute_diff`, which takes
  `Option<&Path>` — the binary did not compile (`E0308`). Fixed with
  `json.as_deref()`; `cargo check --workspace --all-targets` is green
  again.
- `crates/glyim-cli/src/lib.rs` / `crates/glyim-cli/src/linker.rs`: the
  ICE-report writes and the linked-binary chmod silently swallowed their
  `io::Error` (`let _ = ...`); both now surface through
  `tracing::warn!` with the failing path (the write failures decide
  whether a bug report links to a real artifact; the chmod failure
  decides whether the linked binary runs at all).
- **Status renderers (M1.9 first slice)**: `tools/glyim-ledger/src/status.rs`
  renders the M1.8 engine's outcome as the two plan-M1.9 artifacts that
  hang directly off it: the `glyim.status/1` JSON contract (per-level
  histogram + per-feature summary entries; the compute contract keeps the
  `missing`-reason prose) and the marker-delimited Markdown block
  (`<!-- ledger:status:start/end -->`) the "generated content" embedding
  step needs. Both are pure functions of the engine outcome — no clock,
  entries already sorted — pinned by golden tests, a summary-vs-entries
  cross-check, and byte-identical repeat renders. Wired as
  `glyim-ledger status [ROOT] [--evidence PATH] [--format md|json]`,
  with the `status_report` example and the dependency-free `status`
  bench; the "STATUS is up to date" CI gate and the remaining M1.9
  renderers (`KNOWN_GAPS.md` generation, Astro page) are later slices.
- **Level-computation engine (M1.8 slice)**: `tools/glyim-ledger/src/compute.rs`
  implements the plan's `ledger compute` as a pure function of
  *(registry, evidence, policy)*. Per feature it walks the L0–L4 evidence
  ladder (level N requires every rule of levels L1..N; a level whose rules
  are all backend-gated away stops the ladder instead of advancing
  vacuously), applies every open gap's `caps` ceiling (`capped_by` names
  the cappers), and reports the `[hardening]` axes (`fuzz_cpu_hours`,
  `mutation_kill_rate_pct`) as `unmeasured` for L5 targets — no evidence
  kind records them yet, so nothing silently claims hardening. The
  policy schema (`glyim.ledger-policy/1`, `ledger/policy.toml`) lives in
  `tools/ledger-schema/src/policy.rs` beside the other strict registry
  schemas; evidence enters through the `EvidenceSource` seam
  (`ProjectEvidenceSource` over `ledger/evidence/*.jsonl`,
  `JsonlEvidenceSource` for explicit paths, `InMemoryEvidenceSource` for
  tests), reading the harness's `glyim.evidence/1` wire format strictly
  (`deny_unknown_fields`, schema-tag check, hash/mode/backend invariants)
  without a dependency on the test-harness crate. Determinism and
  monotonicity are pinned by golden tests and a deterministic-PRNG
  property test (evidence growth never lowers a computed level); the
  `glyim.ledger-compute/1` JSON contract (`--format json`) and the text
  report render the same outcome. Wired as `glyim-ledger compute [ROOT]
  [--evidence PATH] [--format text|json]`, with the
  `compute_levels` example, the dependency-free `compute` bench, the
  `TOOL-LEDGER-COMPUTE` registry row, and README/CLI docs.
- **Corpus substrate (M2.3 slice 1)**: `tools/glyim-bench/src/corpus.rs`
  adds the seven deterministic synthetic generator families the master
  plan's M2.3 calls for (wide, deep, generic, trait, match, macro,
  closure) as pure functions of `(family, seed, size)` — no clock, no
  ambient PRNG — each mirroring a shape proven by an existing run-pass
  fixture, with the reference exit code derived by construction under
  documented `i32` overflow bounds. The `glyim.corpus/1` manifest
  (`deny_unknown_fields`, byte-deterministic rendering) records family,
  seed, size, exit, compile flags, and the FNV-1a-64 identity of the
  source; `CorpusManifest::verify` regenerates every row and fails with a
  typed `CorpusIdentityMismatch` — the machine-checked "corpora committed
  with hashes" guarantee. New seams carry `tracing` spans
  (`corpus_generate`, `corpus_manifest_build`) and miette help text
  (`CorpusPlan`, `CorpusManifestParse`, `CorpusIdentityMismatch`). Tests
  pin determinism, exit-code contracts, JSON round-trip/strictness, drift
  detection, and FNV vectors; bench `glyim-bench --bench corpus_gen`
  pins build/verify/marginal cost; new consumer example
  `tools/glyim-bench/examples/corpus_walk.rs`; ledger row
  `TOOL-BENCH-CORPUS-GEN`.
- **Micro-debt cleanup**: `glyim-test`'s `TestMode: FromStr` now carries a
  typed `TestModeError` (miette `help()` with the canonical mode list;
  `Display` text byte-identical to the former string error, so substring
  assertions keep passing) instead of `Err = String` — the same debt class
  as the ledger/metrics CLI errors — and `tools/glyim-pilot`'s HTTP
  control-plane bind degrades to a logged error instead of
  `unwrap()`-panicking the detached task (constitution C4).
- **Docs drift fixes**: the README repository map now lists the
  `glyim-ledger gaps` subcommand (landed with the M1.7 witness-debt
  report), records the M2.3 slice-1 state, and lists the `corpus_walk`
  example; the degenerate `/// Struct.` / `/// Variant.` placeholder
  rustdocs in `crates/glyim-test/src/harness/config.rs` are replaced with
  real field docs (the workspace-wide placeholder sweep — ~2.6k sites —
  is tracked as a docs follow-up).
- **Gap registry migration (M1.7 slice)**: the open `KNOWN_GAPS.md` §§1–8
  entries are now registry rows under `ledger/gaps/known/` — the seed's
  GAP-0001/GAP-0002 join **GAP-0003** (§4, `panic!` does not unwind —
  intentional, documented), **GAP-0004** (§6, a partial move out of a
  `Drop`-implementing type suppresses the parent destructor — witnessed) and
  **GAP-0005** (§6, conservative `needs_drop` for `dyn Trait`/projections).
  New `glyim-ledger gaps [ROOT]` subcommand renders the witness-debt report
  (`GapOutcome` / `run_gaps` / `format_gap_report`), with a consumer example
  (`tools/glyim-ledger/examples/gap_report.rs`), a dependency-free bench
  (`benches/gap_report.rs`), the `TOOL-LEDGER-GAPS` registry row, and
  `tracing` spans on the new seam. `KNOWN_GAPS.md` stays the prose audit
  (verification recipes + closed entries) and points at the registry; the
  ~6.1k-line raw session log that had accreted after its Verification
  section moved to `docs/archive/session-log-s16-s18.md`.
- **Measurement harness (M2.2 slice)**: new `tools/glyim-bench` crate turns
  a command (or an in-process workload) into numbers with a stability
  guarantee — the M2.2 definition of done is a *variance* budget, not a
  speed number: instruction-count CoV < 0.1 % over 10 runs.
  - `glyim-bench run [--iterations N] [--source perf|callgrind|wall]
    [--json OUT] -- CMD...` executes the command N times and prints the
    report; `--source perf` wraps the child in `perf stat -x, -e
    instructions,cycles`, `--source callgrind` in `valgrind --tool=callgrind`,
    and the default auto-detects `perf`, falling back to wall-only sampling
    (logged at `info`, never silently). Child RSS comes from
    `getrusage(RUSAGE_CHILDREN)` (Linux; the running maximum over reaped
    children). `glyim-bench gate [--max-cov-pct P] REPORT` re-checks a
    written report against the budget and exits non-zero on failure.
  - New `glyim_bench` library seams: `runner::run_iterated` (one
    `BenchSample` per iteration), `source` parsers for `perf stat -x,` CSV
    and the callgrind `summary:` line, `stats` (min/mean/median/max +
    population CoV, `DEFAULT_MAX_COV_PCT = 0.1`), `alloc::CountingAllocator`
    (opt-in `counting_allocator!` macro; gross allocation events/bytes, the
    columns the M1.5 evidence schema reserved for M2), and `report`
    (`glyim.bench/1` JSON with `deny_unknown_fields`, no clock/PRNG data,
    byte-deterministic rendering — same discipline as the M1.5 evidence
    records).
  - Errors on the new seams are typed `BenchError` diagnostics with
    actionable `help()` text (a missing `perf` suggests `--source wall`; a
    gate failure suggests raising `--iterations`); the runner emits
    `bench_run` spans and per-sample `debug!` events.
  - Tests: CoV vectors, gate boundaries, both counter parsers (with
    `<not counted>` / `<not supported>` handling), report round-trip +
    unknown-field rejection + determinism, and (unix) runner end-to-end over
    `sh -c`. New bench `glyim-bench --bench runner_overhead` pins the
    harness's own cost; new consumer example
    `tools/glyim-bench/examples/measure_alloc.rs` walks the in-process
    counting-allocator path.
  - Ledger: the `tooling.bench` area joins the taxonomy and
    `ledger/features/tooling/bench.toml` declares the harness surface
    (`TOOL-BENCH-RUN`, `TOOL-BENCH-COUNTING-ALLOC`, `TOOL-BENCH-STABILITY-GATE`).
- **Micro-debt cleanup**: `tools/glyim-ledger`'s `parse_args`/`write_schema`
  return a typed `LedgerCliError` and `tools/glyim-metrics`' CLI a typed
  `MetricsCliError` (both `miette::Diagnostic` with per-variant `help()`
  lines) instead of bare `Result<_, String>`; tests now assert on
  constructors rather than message substrings, and CLI output still carries
  the usage text plus a `help:` line.
- **Docs drift fixes**: the README repository map no longer lists the
  already-shipped M2.1 slice as a "next plan item" (it now records the
  landed M2.1/M2.2 state and points at M1.7/M2.3 as next), the "Tracked
  gaps" async bullet no longer claims loop-await state machines are
  implemented (`.await` in a loop body is the diagnosed `[T0060]` hard
  error per `KNOWN_GAPS.md` §1), the member count is 39 (34 + 5 tooling)
  with a `tools/glyim-bench` row, `docs/TECHSTACK.md` re-anchors at
  workspace 0.7.1 (`ba2a526e`) and records the `miette`/`clap`/`tracing`
  consumer additions and the `libc` per-crate extra.
- **Per-phase instrumentation (M2.1 slice)**: every pipeline phase
  (load → parse → expand → defmap → hir → typeck → lower → borrowck →
  opt → mono → codegen) now opens a `tracing` span and accumulates
  wall-clock microseconds into a `glyim_pipeline::PhaseProfile`, exposed
  on `CompileArtifacts` / every emit path.
  - `glyim` CLI: `--time-passes` prints the fixed-width per-phase table
    on stderr; `--self-profile[=FILE]` writes a Chrome Trace event list
    (Perfetto / `chrome://tracing`-openable; value is `=`-bound so the
    positional INPUT can never be swallowed, bare form defaults to
    `./trace.json`); `--emit=stats` runs the analysis-only front half and
    writes a schema-versioned (`version: 1`) JSON report with the phase
    table plus mono/warning counters. A compile-cache hit records no
    phases and says so instead of printing an empty table.
  - New `glyim_pipeline::Pipeline::compile_stats` public API backs
    `--emit=stats`; `emit_mir`/`emit_syntax`/`emit_llvm_ir`/`emit_asm`
    now return the profile so every emit mode can honour
    `--time-passes`/`--self-profile`.
  - Determinism contract (unit-tested): the phase set/order is pinned,
    all three renderers are byte-deterministic for a given profile, and
    an empty profile renders a valid zeroed report (no NaN).
  - Errors on the new seams carry miette help text (e.g. an unwritable
    `--self-profile` path suggests the remedy) and the write path emits
    `tracing::debug!` events.
  - New consumer example `crates/glyim-pipeline/examples/phase_profile.rs`
    drives `compile_stats` + all three renderers as a library consumer;
    `hello_pipeline` now also prints the phase table. New bench
    `glyim-pipeline --bench phase_prof` measures the renderer cost
    (sub-µs per render — the instrumentation gate is effectively free).
- **Micro-debt cleanup**: `glyim-test`'s fixture-header parser
  (`harness::config::parse_test_config`) returns a typed
  `TestConfigError` (miette `Diagnostic` with an actionable `help` line
  naming the `//@` grammar and the tag codemod) instead of a bare
  `String`; the stale rustdocs in `glyim-hir/src/lower/lower_async.rs`
  that claimed the multi-await v1 desugar was an unwired scaffold (and
  referenced a nonexistent `allow(dead_code)`) now describe the live
  dispatch; the malformed `KNOWN_GAPS` reference in the
  `glyim-mir-interp` crate docs is fixed.
- **Docs drift fixes**: README's `glyip` quickstart no longer documents a
  nonexistent `-O2` flag (`glyip build` takes `--opt-level`), the CLI
  flag table gains `--with-stdlib`, `-A/-W/-D`, `--emit=syntax`, and the
  new M2.1 measurement flags, the stdlib module lists match the actual
  `lib/` directories (`parse`, `alloc`, `int_str`), the "Tracked gaps"
  section no longer claims cross-frame unwinding is missing (it is
  implemented and hardened — see `KNOWN_GAPS.md`), and the repository
  map describes the real milestone state (M0 landed, M1.1–M1.6 landed)
  instead of "M0 slice". `docs/TECHSTACK.md` now records `async-lsp` in
  the §2 table (previously claimed absent while `glyim-lsp` depends on
  it), fixes the `clap` consumer list (`glyim-cli`/`glyip`/
  `glyim-pilot` — not `glyim-ledger`), corrects the
  `reqwest`/`flate2`/`tar` versions, updates `miette`'s consumers, and
  re-anchors its source-of-truth header at workspace 0.7.0.
  CONTRIBUTING no longer claims `RUSTC_WRAPPER=kache` is set in the
  default dev environment (it is maintainer-machine-specific), and the
  stale `glyim-test` manifest note about the (now closed) async
  `fn_abi_of` codegen gap is reconciled with CI.
- **Evidence JSONL emitter (M1.5 slice)**: `glyim-test` now emits one
  readiness-ledger evidence record per executed fixture when
  `GLYIM_TEST_EVIDENCE` names an output path — the machine-checked
  input `ledger compute` (M1.8) will consume, per plan §3.4/§6 M1.5.
  - `harness::evidence`: typed `EvidenceRecord` (`glyim.evidence/1`) with
    outcome/backend enums from the ledger's backend set, SHA-256 fixture
    hashes over the exact source bytes, mode, revision, declared
    `//@ feature:`/`//@ backends:` tags, and stable short reason classes
    (free-form compiler output deliberately dropped so message refactors
    cannot move ledger statistics). `instructions`/`rss_kb` are reserved
    (always `None` until M2's counting-allocator instrumentation).
  - Hermetic by construction: no clock or PRNG data —
    `TestResult::duration` never reaches a record, and two runs against
    the same fixtures serialise byte-identically (tested). Records
    deserialise strictly (`deny_unknown_fields`, same discipline as
    `ledger-schema`), and `write_evidence` validates the whole slice
    before writing so a schema bug cannot poison the evidence store.
  - `tracing` spans (`evidence_write`, record/byte count events) on the
    emission path, matching the harness's existing span style; failures
    render through `miette` with actionable `help` text (the remedy names
    `GLYIM_TEST_EVIDENCE` or the schema rule that was violated).
  - Bench: `glyim-test --bench evidence` measures a full 375-fixture
    evidence pass (record construction ≈0.6 ms, serialisation ≈0.2 ms,
    validated write ≈0.6 ms) — the M1.5 gate is effectively free.
  - New consumer examples: `crates/glyim-test/examples/evidence_report.rs`
    round-trips the JSONL contract (producer + validator + reason-class
    aggregate) and `crates/glyim-pipeline/examples/hello_pipeline.rs`
    drives the compiler as a library (write a `.g` program, lower it to
    MIR in-process).
- **Micro-debt cleanup**: the harness JSON reporter no longer swallows
  failures — `GLYIM_TEST_JSON` write/serialise errors now print a
  miette-rendered diagnostic instead of vanishing into `let _ =`; the
  dead workspace dependencies `textwrap`, `owo-colors`, and `petgraph`
  (declared in `[workspace.dependencies]`, referenced by no member,
  absent from `Cargo.lock`) are removed; the stale committed
  `Cargo.lock` (members at 0.5.1 vs workspace 0.6.0) is regenerated.
- **Docs drift fixes**: README's architecture table gains the missing
  `glyim-gsx` row and a tooling row for `tools/*` (the workspace has 38
  members: 34 crates + 4 tools); CONTRIBUTING says Rust ≥ 1.95 (was
  1.85), replaces the inline-`#[allow]` guidance with the M0
  zero-allow policy (relaxations only in `[workspace.lints.clippy]`),
  documents the M1.4 `//@` fixture grammar instead of the legacy
  `// key:` headers, and updates the corpus count (~260 → ~410);
  `docs/TECHSTACK.md` is rewritten (v2.0.0) to list only dependencies
  the manifests actually declare (the previous revision listed
  `salsa`/`chumsky`/`egg`/`regalloc2`/`async-lsp` and nonexistent
  crates); the master plan's F1 evidence pointer now reflects that the
  production-readiness playbook has been retired from `docs/roadmaps/`.
  The M0 metrics ratchet baseline is refreshed (+1,041 code lines, +10
  test functions, +16 documented items, 0 inline allows).
- **Fixture annotation grammar + codemod (M1.4)**: every `.g` fixture
  carries a machine-readable `//@` header, and the harness rejects
  fixtures that lack one — the "100% of fixtures tagged; fixtures lacking
  expectations rejected" DoD is a tested fact, not a promise.
  - `glyim-test`: new `annotations::directives` grammar — `//@ mode:`,
    `//@ feature:` (comma-separated registry IDs, shape-checked),
    `//@ backends:` (validated against the ledger's `gam`/`llvm`/
    `cranelift`/`bytecode` set), the `expect-*` outcome family
    (`expect-exit`/`expect-stdout`/`expect-stderr`/`expect-error`/
    `expect-warning`/`forbid-warning`), and the harness gates
    (`needs-llvm`, `ignore`, `only-target`, `compile-flags`, `aux-file`,
    `timeout`, `min-version`, `revisions`, `[rev] compile-flags`).
    Unknown keys, malformed IDs, duplicate singular keys, and mixing a
    legacy `// key:` header with its canonical `//@` spelling are hard
    parse errors, so a typo can never become a silent no-op tag.
  - `glyim-test`: the collector rejects fixtures with no `//@` header
    (`MissingAnnotations`) and fixtures whose mode lacks the required
    expectation — run-pass/run-fail must pin an exit code or
    stdout/stderr pattern (the strategies' defaults `0`/`1` are now
    spelled out in every fixture), compile-fail must carry an inline
    `//~` annotation or `//@ expect-error:` (`MissingExpectation`).
  - `glyim-test`: new `glyim-tag-fixtures` codemod bin converts every
    fixture header to the canonical form and derives the ledger tags
    (410 fixtures: 375 in the `glyim-test` corpora plus the 35 in the
    per-crate corpora of `glyim-lower`/`glyim-typeck`/`glyim-pipeline`/
    `glyim-lang-{alloc,core,std}` that drive the same `TestCollector`):
    `feature` from a curated keyword table (92 distinct IDs, all
    cross-checked against the M1.2 registry via `--verify-ids`) plus the
    fixture-mode feature, `backends` from the CI reality (`gam, llvm`
    for the run corpora and `needs-llvm` fixtures; `gam, bytecode` for
    the `--backend=bytecode` fixture; `gam` elsewhere). The rewrite is
    idempotent; prose comments pass through untouched.
  - The tagging surfaced two real seams: `runtime/drop_order.g` (a
    bytecode-backend fixture not driven by any corpus — the async-proof
    gating in `tests/runtime.rs` covers `runtime/m5` only) and the
    `typeck/` + `codegen/` fixture dirs (no corpus root points at them).
    Both are now tagged and recorded for the ledger's work queue (M1.9
    `ledger next`), not silently fixed here.
  - Bench: `glyim-test --bench annotations` measures one full discovery
    pass (parse: 375 rich headers in ~0.4 ms; codemod rewrite: 375
    legacy fixtures in ~1.4 ms) — the M1.4 gate is effectively free.
  - New tests: 29 unit tests across the grammar contract and the codemod
    rewrite rules, plus the workspace-wide `annotations_corpus` DoD gate
    (covering the per-crate corpora as well). The M0 metrics ratchet
    baseline is refreshed (+129 test functions, 0 inline allows).
  - Docs drift fixed while landing: README badge/MSRV said Rust 1.94
    (workspace `rust-version` is 1.95), the crate-count badge said 20+
    (the workspace has 38 members), and the stale committed `Cargo.lock`
    (members at 0.5.0 vs workspace 0.5.1) is regenerated.
- **Inventory extractors, slice 1 (M1.3)**: `ledger inventory` reads the
  source, so the registry can no longer drift from the code (plan §3.5).
  - `ledger-schema`: two `syn`-based extractors — MIR IR variants
    (`mir.<Enum>.<Variant>` for the five backend-parity enums
    `StatementKind`/`Rvalue`/`Operand`/`ProjectionElem`/`TerminatorKind`
    in `crates/glyim-mir`) and runtime FFI symbols
    (`runtime.ffi.<symbol>` for every `#[no_mangle]`/`#[unsafe(no_mangle)]`
    `pub extern "C"` fn in `crates/glyim-runtime`, 99 of them). Feature
    entries gain an optional `inventory = [...]` claim list; a new
    `ledger/inventory-ignore.toml` registers deliberate non-features
    (exact keys only — no globs, so a new variant can never be silently
    swallowed). `inventory_diagnostics` fails `ledger check` on unmapped
    items (`LEDGER-INVENTORY-UNMAPPED`), stale ignores
    (`LEDGER-INVENTORY-STALE-IGNORE`), stale claims
    (`LEDGER-INVENTORY-STALE-CLAIM`), duplicate claims/ignores
    (`LEDGER-INVENTORY-DUP-CLAIM`, `LEDGER-INVENTORY-DUP-IGNORE`),
    claimed-and-ignored conflicts (`LEDGER-INVENTORY-CONFLICT`), and
    unparseable sources (`LEDGER-INVENTORY-PARSE`) — the M1.3 DoD
    "adding a MIR variant without registration fails `ledger check`
    (tested)" is a unit + integration tested fact.
  - `glyim-ledger`: new `ledger inventory [ROOT]` subcommand (items with
    claim/ignore status, exit 1 on gate errors); `ledger check` now runs
    the inventory gate and reports the extracted item count.
  - Registry seed: the 15 implemented FFI families claim their 99
    symbols; the 29 existing MIR variants are ignored-with-reason (the
    per-variant rows are the §3.6 backend-parity matrix, M1.8). The
    extractor immediately surfaced registry drift: `RUNTIME-FFI-FS`
    claimed 16 symbols in its title but implements 14 (title corrected);
    `RUNTIME-FFI-SYNC` (mutex/rwlock) has no backing symbols at all and
    stays unclaimed until they land.
  - Bench: `extract_inventory` harness measures parse+extract of a
    synthetic 150-variant/100-symbol corpus (~1.4 ms, parse-dominated).
  - 31 new tests across the two ledger crates, written test-first.
- **Feature registry seed (M1.2)**: the ledger's claim surface grows from
  6 smoke-test entries to 216 curated features across 32 registry files,
  covering the documented inventory end to end: the README feature list
  and language tour, `docs/specs/v0.1.0.md`, all eight `docs/design/*`
  documents, the 33 stdlib modules of `glyim-lang-core`/`-alloc`/`-std`,
  the 16 runtime FFI symbol families, and the tooling surface (CLI,
  `glyip`, LSP, test harness, and the ledger/metrics tooling itself).
  - `ledger-schema`: the feature taxonomy the master plan delegates to an
    "Appendix C" that was never checked in (docs drift found during the
    M1.2 deep dive) now lands in-repo as `TAXONOMY` — 34 areas over six
    namespaces (`lang.*`, `compiler.*`, `codegen.*`, `tooling.*`,
    `std.*`, `runtime.*`), each with an enforced ID prefix — plus
    `min_target_level` encoding release gate RG-09 (`core` ⇒ L5,
    `std` ⇒ L4) at declaration time. `ledger check` now fails on unknown
    areas (`LEDGER-AREA-UNKNOWN`), prefix mismatches
    (`LEDGER-ID-AREA-MISMATCH`), tier/target violations
    (`LEDGER-TIER-TARGET`), and dead spec references
    (`LEDGER-SPEC-MISSING`): every entry's `spec` must resolve to a real
    repo-relative file, so a moved or renamed doc breaks CI instead of
    silently rotting the registry.
  - `ledger-schema`: new `load_project_registry` production entry point —
    `glyim-ledger check <ROOT>` walks `ROOT/ledger` and anchors
    repo-relative references (`spec` paths, gap witnesses) at the project
    root, matching the plan's path examples; the fixture-facing
    `load_registry` keeps its behavior for tests.
  - `glyim-ledger`: the check gate runs the taxonomy/spec validation and
    reports a `bytecode` backend variant for the
    `glyim-codegen`/`glyim-bytecode-vm` executor (goes away with
    ADR-004's removal, if accepted).
  - Bench: the `ledger-schema` load+validate harness is scaled to the
    registry's real size (300 features / 50 gaps synthetic, was
    200/40), switched to the production `load_project_registry` path and
    fixed to a taxonomy-valid fixture, so CI headroom tracking measures
    the M1.2 shape: ~2.6 ms per load+validate of 350 entries.
  - Seed data fixes found by the new gates: `LANG-ASYNC-AWAIT` now
    targets L5 as RG-09 requires for `core` tier entries.
  - 65 tests total in the two ledger crates (14 new taxonomy/spec tests
    in `ledger-schema`, 2 new check-integration tests in
    `glyim-ledger`), written test-first.
- **Readiness Ledger v1 foundation (M1.1)**: new `tools/ledger-schema`
  and `tools/glyim-ledger` crates implementing the registry schema half
  of the master plan's Workstream A (§3).
  - `ledger-schema`: strict TOML entry types (`[[feature]]`, `[[gap]]`)
    with `deny_unknown_fields` — typos are load errors, never silently
    ignored keys; the maturity-level/gap-class/backend/lane vocabulary
    from the plan (§3.2, §3.3, §3.6, §3.8); `load_registry` walking
    `ledger/features/**` and `ledger/gaps/**` deterministically (sorted
    walks, sorted diagnostics); `Registry::validate` — the `ledger check`
    engine (unique IDs, `affects`/`requires` reference resolution,
    `requires` cycle detection, and the strict-xfail witness contract
    with `//@ known-gap:` headers and ratcheted witness debt); and a
    dependency-free JSON-Schema (draft-07) export for both entry types.
  - `glyim-ledger`: `ledger check [ROOT]` registry gate (exit 1 on any
    error, witness debt reported as warnings) and `ledger schema [OUT]`
    JSON-Schema export. Hand-rolled argument parsing, matching the
    `glyim-metrics` no-clap convention. 49 tests total, written
    test-first, plus a load+validate throughput bench.
  - Seed registry `ledger/`: 6 real feature entries (control flow,
    pattern matching, generics, async, GSX views, unwind cleanup) and 2
    real gap entries migrated from `KNOWN_GAPS.md` (GAP-0001 await-in-loop
    class a, GAP-0002 SEH cleanup unreachable class b), both declared
    `unwitnessed` until the M1.6/M1.7 witness fixtures land.
  - CI: new `ledger-check` job runs the gate on every PR (pure Rust, no
    LLVM dependency).
- **GSX view syntax** (docs/design/GSX-in-Glyim.md): first-class, typed,
  host-agnostic view syntax with a dedicated compiler stage.
  - Frontend: `Gsx*` CST nodes and `parse_gsx_element` (elements, fragments,
    self-closing, directive attrs `on:`/`class:`/`style:`/`bind:`, spread,
    shorthand, hyphenated names, `for … by` keyed iteration, text holes split
    at parse time; zero lexer changes).
  - HIR: `Expr::Gsx` preserves element structure (no call-tree desugar).
  - Typeck: `check_gsx` validates tags/props/events/children against dialect
    schemas and checks hole expressions.
  - New crate `glyim-gsx`: View IR (skeleton + slots + regions), flat-`u32`
    blueprints, read/write-set analysis with prefix interference,
    mutation-specialized patch functions (§7.5), SSR emitter, Host trait +
    headless op-recording host, keyed-list LIS reconcile, `Signal<T>` (G0201)
    diagnostics with fix-its. 88 crate tests including the §3.1 Counter
    example end-to-end.
- Fuzzing infrastructure: `fuzz_parse` (lexer/parser/CST round-trip) and
  `fuzz_compile` (full-pipeline ICE detector) cargo-fuzz targets with
  corpus seeds and a token dictionary.
- CI: strict M5 runtime proof (real LLVM codegen), differential suite
  (LLVM native vs MIR interpreter), lint job (`fmt` + `clippy -D
  warnings`), cargo-deny supply-chain job, runner caching, timeouts,
  concurrency cancellation.
- MIT LICENSE file, declared MSRV (`rust-version = "1.85"`).
- **M0.6 metrics ratchet**: new `tools/glyim-metrics` crate (TDD, 18 unit
  tests + dependency-free scan bench) measures lexical repo health —
  `.rs` files, code lines, public items, doc coverage, test functions,
  and the inline-`allow` count the M0 zero-allow policy pins at zero.
  The binary writes `metrics/baseline.json` (906 files / 226.5k lines /
  4811 tests / 0 inline allows at M0) and CI's new `metrics-ratchet` job
  fails on any regression while allowing improvements (gates never
  weaken, C9).
- **Workspace-wide pedantic lint rails** (M0 §2 C4, §7.4):
  `[workspace.lints.clippy] pedantic` is wired into all 36 member crates
  via `[lints] workspace = true`; every relaxation is a single
  rationale-annotated entry in the root manifest, and the enforced gate
  (`cargo clippy --workspace --all-targets --all-features -- -D warnings
  -W clippy::pedantic` with manifest-derived re-asserts) passes with
  **zero inline `#[allow]`/`#[expect]` attributes** in the tree.
- `rust-toolchain.toml` pins local development to the `stable` toolchain
  (MSRV 1.95.0 stays a separate CI job).

### Fixed
- **`glyim-opt`'s drop-flag statement inspection re-read the pushed
  statements through `new_stmts.last().unwrap()`** — provably non-empty,
  but the re-inspection also saw the optional plain-flag statement pushed
  one line earlier, so the per-element check's input depended on push
  order. The per-element flag statement is now computed from the original
  statement before it is moved (branch is total; emitted order fixed:
  original stmt, plain flag, per-element flag). Behaviour-preserving; the
  `COMPILER-OPT-DROP-ELAB` fixtures pin it.
- **GSX SSR emitter dropped every element's closing `>`** (`</h1` instead of
  `</h1>`) — the crate's own G7 snapshot tests were left red on `main` (the
  ubuntu `Test` job has been failing since); the fix makes all 88 `glyim-gsx`
  tests pass.
- **`glyim-test`'s `json-output` feature did not compile when enabled**: the
  optional `serde` dependency lacked its `derive` feature, so the feature's
  `#[derive(Serialize)]` blocks failed (`cargo test --all-features` could not
  build the crate). Found by the M1.3 gate run.

### Changed
- **The strict-xfail witness contract covers run modes (M1.7)**:
  `//@ known-gap: GAP-NNNN` is now accepted on `run-pass`/`run-fail`
  fixtures (a pinned exit code is a valid gap-open behaviour witness) and a
  failing run witness sharpens into `FailureReason::KnownGapClosed` instead
  of a generic exit/output mismatch. `compile-pass`/`ui` witnesses stay
  rejected at discovery — the discovery error is renamed
  `WitnessRequiresCompileFail` → `WitnessUnsupportedMode` with updated
  guidance. First consumer:
  `tests/run-pass/behavior/partial_move_drop_guard_known_gap.g` witnesses
  GAP-0004 on the exit-code contract.
- **`regalloc2` dependency removed** from the workspace (unused).
- Session documents `HANDOFF.md` and `FIXES.md` moved to `docs/archive/`
  to keep the repo root navigation-focused.
- Dead code deleted across the workspace: unused fields/methods/helpers
  (`DoneDefaults` fields, `record_commit`/`set_provider_cooldown`,
  `SourceMap::path`, `AnalysisDriver::cache_dir`, test-only helpers that
  no test reached, …) — replaced with `#[cfg(test)]` where the consumers
  are the test suites themselves.
- ~160 `missing_docs` gaps closed; `push_str(&format!(..))` idioms
  converted to `write!`/`writeln!`; format args inlined per
  `uninlined_format_args`.
- CI installs LLVM 23 consistently (`./llvm.sh 23` — previously mixed
  v22 script with v23 dev packages).
- Release profile enables `overflow-checks` (compiler-internal arithmetic
  overflow is an ICE, never a silent wrap).

### Fixed
- `parse_type` recursion depth unbounded → deeply nested types crashed the
  compiler with an uncatchable stack overflow (SIGSEGV). Now a parse
  diagnostic (`MAX_EXPR_DEPTH` guard, same policy as expressions).
- MIR interpreter subslice projection: bounds guard checked `to` while
  subtracting `from` → `usize` underflow (panic in debug, bogus slice
  length in release). Both operands are now validated.
- MIR interpreter `byte_payload`: program-controlled `repeat(n)` could
  OOM the host; payload capped at 16 MiB.
- const-eval array/tuple index: `u128 → usize` truncation silently aliased
  indices ≥ 2^64 to in-bounds elements. Oversized indices now error.
- Interpreter internal index accesses (`basic_blocks`, argument locals)
  return `InterpError` with a "please report" message instead of panicking.
- `glyim-cli` ICE report now includes the panic message, suppresses the
  default panic printer during `catch_unwind`, and points to the correct
  issue tracker.
- `glyip test` isolates interpreter panics per fixture instead of aborting
  the whole test session.
- LLVM codegen: recoverable type/ABI failures return diagnostics instead
  of panicking (`fn type lowering failed`, `Codegen error`, variant index).
- README: correct repo URL, binary name, and LLVM-install guidance.
- Legacy session instrumentation and stale doc-comment artifacts
  (unbalanced backticks in doc comments, detached docs, duplicated
  crate-level docs) cleaned up as part of the M0 doc pass.

## [0.1.0] — initial public state

Research-grade end-to-end compiler: lexing, parsing, name resolution,
macros, HIR, type inference + trait solving, NLL borrow checking, MIR
optimizations, LLVM 23 backend, bytecode backend + VM, MIR interpreter,
LSP, `glyip` build tool, and a five-mode test harness.
