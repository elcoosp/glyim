//! # glyim-gsx — the GSX view pipeline
//!
//! GSX (docs/design/GSX-in-Glyim.md) is Glyim's first-class, typed,
//! host-agnostic view syntax: JSX's ergonomics, but a *compiler-visible
//! structure* instead of a runtime function-call tree. This crate is the
//! dedicated compiler stage above MIR lowering that the design doc's §4
//! prescribes as the new crate `glyim-gsx`:
//!
//! 1. **Syntax** (`ast`, `expr`): the `Gsx*` CST nodes produced by
//!    `glyim-frontend` are converted into a structural GSX AST; interpolation
//!    holes are split from text literals at parse time (§3.2) and slot/handler
//!    expressions are reduced to a small expression model (`ExprModel`) whose
//!    reads and writes are syntactically visible.
//! 2. **Schemas** (`schema`): elements are *data described by schemas* — the
//!    compiler passes know schemas, slots, regions and invalidation classes;
//!    they never contain the string `"div"` (§2, principle 2). Hosts and
//!    dialects are libraries.
//! 3. **VIR** (`vir`): the View IR — static skeleton + slots + regions
//!    (§6). Structure is never erased early; `<div class="a">{x}</div>`
//!    survives as a tree of static ops, typed holes and control-flow regions.
//! 4. **Blueprint** (`blueprint`): the static skeleton is encoded as a flat
//!    `u32` op array plus a string table in the shape of a `static` (§6.1,
//!    finding F10 — `MirConst` has no aggregate constants, so only integer
//!    arrays are needed).
//! 5. **Ownership-driven reactivity** (`analysis`, `msp`): state is owned
//!    data; views read it by shared borrow, handlers mutate it by exclusive
//!    borrow (§7.2). Read-sets of slots and write-sets of handlers are
//!    computed from the expression model, interference is prefix-based
//!    (§7.4), and for each distinct handler write-set the compiler emits one
//!    straight-line **mutation-specialized patch function** (§7.5) — no dirty
//!    flags, no diffing, no subscriptions on the fast path.
//! 6. **Emitters** (`emit`): artifacts per host *flavor* (§5.4): stream hosts
//!    get SSR `render_to` only (no patch code at all, G17), retained hosts get
//!    `mount`/`patch_*`/`unmount`, and hydration islands (G16) are generated
//!    only for regions that actually need them.
//! 7. **Hosts** (`host`): the `Host` trait plus a headless, op-recording test
//!    host that makes the §8.1 cost model *assertable in CI from day one*
//!    (§11.4): one host call per updated text slot, zero visits for static
//!    subtrees, zero per-handler allocations (static fns + inline env, G8).
//!
//! The crate is host-agnostic on purpose: it contains no HTML strings in the
//! passes — the `web` dialect is *data* in `schema` like any other.
//!
//! ## Module map (design doc cross-reference)
//!
//! | Module | Design section |
//! |---|---|
//! | [`diag`] | §7.7 diagnostics (`G0201`, …), §5.2 schema violations |
//! | [`ast`] | §3.1–§3.4 syntax, text-hole splitting |
//! | [`expr`] | §7.4 read/write sets over slot + handler expressions |
//! | [`interp`] | §7.3 handler semantics (state transformers), §7.9 `bind:` |
//! | [`schema`] | §5.1–§5.3 Schema / Dialect / invalidation classes |
//! | [`vir`] | §6.1–§6.3 Blueprint + slots + regions, G2, G9 |
//! | [`blueprint`] | §6.1 op encoding, F10 |
//! | [`analysis`] | §7.4 read/write sets, prefix interference, §7.6 indexed |
//! | [`msp`] | §7.5 MSP patch functions, masks, dispatch |
//! | [`emit`] | §6.4 artifacts, G7 SSR, G16 islands, G17 flavors |
//! | [`host`] | §5.4 Host trait, §7.6 keyed regions, §7.7 Signal bits |

pub mod analysis;
pub mod ast;
pub mod blueprint;
pub mod diag;
pub mod emit;
pub mod expr;
pub mod host;
pub mod interp;
pub mod msp;
pub mod schema;
pub mod vir;
