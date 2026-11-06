# GSX in Glyim — Architecture, Ideas & Roadmap

> **GSX** is Glyim's first-class, typed, host-agnostic view syntax: JSX's ergonomics, but a *compiler-visible structure* instead of a runtime function-call tree. It targets HTML, SSR strings, native widgets, terminals, GPU scenes and headless test hosts from the same source.

This document is grounded in a read of the repository at `github.com/elcoosp/glyim` (shallow clone, ~233k lines of Rust, 33 crates). Section 1 lists what I actually found, because several design decisions below are driven by it. Everything else is design proposal, and I mark confidence and prior art honestly in §14.

---

## Table of contents

1. [What the codebase tells us](#1-what-the-codebase-tells-us)
2. [Thesis: GSX is a typed DSL substrate, not sugar](#2-thesis)
3. [Syntax](#3-syntax)
4. [The pipeline: where GSX lives](#4-the-pipeline)
5. [The core abstractions](#5-the-core-abstractions-host-dialect-schema-view)
6. [The compilation model: static skeleton + dynamic slots](#6-the-compilation-model)
7. [The headline idea: ownership-driven reactivity](#7-the-headline-idea-ownership-driven-reactivity)
8. [Performance catalog](#8-performance-catalog)
9. [Native GUI, not just web](#9-native-gui-not-just-web)
10. [Metaprogramming: GSX as the first comptime customer](#10-metaprogramming)
11. [Developer experience](#11-developer-experience)
12. [Roadmap mapped to the repo](#12-roadmap-mapped-to-the-repo)
13. [Risks and open questions](#13-risks-and-open-questions)
14. [Competitive positioning & honesty about prior art](#14-competitive-positioning)
15. [Decisions I need from you](#15-decisions-i-need-from-you)

---

## 1. What the codebase tells us

### 1.1 Pipeline as it exists

```
source.g → Lexer (eager, context-free) → Parser (rowan CST) → Macro expansion (token trees)
        → Def-map → HIR (arena Expr ids) → Typeck/solve → THIR → Lower+mono → MIR (CFG)
        → Borrowck (NLL) → Opt (per-body passes) → LLVM | bytecode VM
```

### 1.2 Findings that shape GSX

| # | Finding (evidence) | Consequence for GSX |
|---|---|---|
| F1 | **No GSX/JSX anywhere** (`grep -ri "gsx\|jsx"` finds nothing). Greenfield. | Freedom to design the IR properly instead of retrofitting. |
| F2 | **`comptime` is only a keyword.** `KwComptime` is lexed (`glyim-frontend/src/lexer.rs:956`) and listed in `SyntaxKind`, but nothing in the parser, HIR, typeck or pipeline consumes it. | The comptime VM is not available yet. GSX v0 must **not depend on it**; it should be a built-in compiler pass that later *opens up* to comptime extension (§10, §12). |
| F3 | **Design docs and README disagree about macros.** README advertises `macro_rules!` + proc macros; `docs/design/macros-comptime.md` says "`macro_rules!` is not supported", and `ucms.md` plans a new `glyim-cvm` crate that does not exist in `crates/`. | The macro direction is committed on paper but not in code. GSX is a strong first real customer to force the comptime design to be concrete. |
| F3b | `docs/design/phases.md` proposes a `glyim-comptime` partial-evaluation phase built on `glyim-mir-interp`. | Two competing comptime designs (CVM-on-HIR vs interp-on-MIR). GSX analyses mostly need *structural* comptime (on VIR), which favors the CVM/token-stream route. |
| F4 | **The lexer is eager and context-free** (`Lexer::lex_all` tokenizes the whole file up front; no mode stack). | Classic JSX *raw text children* (`<p>Don't stop</p>`) would need lexer modes. **Design choice: GSX v1 text is always a string literal** (§3), which needs *zero* lexer changes and removes JSX's whitespace footguns. |
| F5 | **`<` in expression-prefix position is unclaimed.** `parse_primary_expr` has no `Lt` arm, and there's no qualified-path `<T as Trait>::f` support (only `as` casts). Binary `<` only occurs in infix position. | A prefix `<` followed by an identifier/`>` is unambiguously a GSX element today. If qualified paths are added later, disambiguate by lookahead (`<Ident as` / `<Ident>::`). |
| F6 | **Macro expansion is token-tree based**, and token trees group only `() [] {}` (`glyim-meta/src/expander/token_tree.rs`). `<...>` is not a group. | Don't implement GSX as `macro_rules`/proc-macro over token trees. Make it **first-class syntax with its own CST nodes** (as Rust's `rsx!` ecosystem cannot). `gsx! { }` may still exist as an escape hatch. |
| F7 | **HIR is an arena of `Expr` variants** (`glyim-hir/src/lib.rs`: `Path, Literal, Block, If, While, Loop, For, Match, Call, MethodCall, …`). | Add `Expr::Gsx(...)`. Do **not** desugar to `Call` early (the `createElement` mistake). Keep structure through typeck. |
| F8 | **MIR is a plain CFG** with `Assign(Place, Rvalue)`, `Place { local, projection[] }`, `Aggregate(…Closure…)`, `Repeat`, `Call` terminators. `Place` projections are *field-granular*. | Place-level read/write sets are directly computable → the basis of ownership-driven reactivity (§7). |
| F9 | **`glyim-opt` has no inliner or interprocedural pass.** Passes: `slice_desugar, constant_prop, dce, cfg_simplify, unreachable_elim, drop_elaboration`, all per-body. Inlining is left to LLVM. | GSX specialization (component inlining, prop specialization, patch generation) must run **above MIR** on VIR/THIR, where structure and call relationships are still visible. Don't count on MIR-level cleanup to rediscover it. |
| F10 | **`MirConst` has no aggregate constants** (`Int, Uint, FloatBits, Bool, Char, String, Unit, Fn, ConstRef, StaticRef`). Statics carry const-evaluated initializers, but KNOWN_GAPS notes const-eval limits for non-local paths. | Encode the Blueprint as a **flat `u32` op array + string table** in a `static`. It needs only integer-array statics, not rich const aggregates. |
| F11 | **Core traits exist**: `Fn/FnMut/FnOnce`, `Drop`, `Deref`, `Index`, `Future`, plus closure capture analysis (`ByValue/ByRef/ByRefMut`) and async state-machine desugar. | Enough to define `View`/`Host` traits and handlers. Async `.await` in loops is a diagnosed gap (`[T0060]`), so GSX async data (`<Suspense>`) must avoid loop-awaits initially. |
| F12 | **Known open bug: associated-type projection.** HANDOFF §3.9: `TyKind::Projection reached LLVM codegen`, and `&[i32].into_iter()` fails with `[T0001]`. | **Prerequisite P0.** A trait-based `Host` with associated types (`type Node`) leans on projection normalization. Either fix it first or design `Host` with generic params instead of associated types for v0. |
| F13 | **`panic!` aborts, no unwinding** (exit 101). Partial moves leak siblings (sound, over-retains). | Fine for UI. Means no unwinding-safety burden on view code. Partial-move leak argues for arena-allocated node trees (§8, G10). |
| F14 | **Incrementality**: SHA-256 source/config fingerprinting, `glyim-pipeline/mono_cache.rs`, `glyim-db` mono cache; polymorphization pass in `glyim-lower`. | Hooks for per-blueprint caching, hot reload, and "prop polymorphization" (G9). |
| F15 | **Two backends**: LLVM and a bytecode VM (`glyim-codegen` + `glyim-bytecode-vm`), plus MIR interpreter. | Uncommon asset: dev builds can run blueprints via the VM for instant hot-reload; release builds AOT via LLVM. Same VIR feeds both (§11). |
| F16 | **Testing infra is strong**: `glyim-test` with `//~ ERROR` annotations, snapshot modes (CST, def-map, MIR), mocks per phase. LSP exists with completion/hover/code actions. | Cheap to add VIR snapshots, GSX diagnostics tests and schema-driven LSP completion. |
| F17 | WASM is referenced in `glyim-runtime`, `glyim-cli/linker.rs`, `glyim-codegen-llvm/{abi,types}.rs`, but I did not verify an end-to-end wasm target. | **First hosts should be non-wasm** (SSR string host, headless test host), then native, then web/wasm. See §12. |

---

## 2. Thesis

**React's mistake, inherited by JSX-in-every-language, is that the view is a runtime function-call tree the compiler can't see into.** Every optimization (Solid, Svelte, Compose, Dioxus templates, Vue Vapor) is a compiler *recovering* structure that syntax-sugar threw away.

Glyim owns the compiler, the borrow checker, the type system and (planned) comptime. So GSX should invert the arrangement:

> **GSX is a typed *view IR* that survives to the middle of the compiler.** The compiler sees the static skeleton, the dynamic holes, who can mutate what, and which host will render it. Elements are *data described by schemas*, not strings.

Three principles fall out:

1. **Structure is never erased early.** `<div class="a">{x}</div>` stays an `Expr::Gsx` through HIR and typeck; only a dedicated pass lowers it to MIR.
2. **The compiler is host-agnostic; hosts are libraries.** Passes know *schemas, slots, regions, invalidation classes*. They do not know HTML. HTML, native, terminal are all *dialects + hosts* plugged in.
3. **Ownership is the reactivity system.** Glyim already proves who can write to what; use that instead of runtime signals/vdom diffing where possible (§7).

---

## 3. Syntax

### 3.1 A complete example

```glyim
use ui::{View, Host, Handler};

#[state]
struct Counter {
    count: i32,
    step:  i32,
    name:  String,
}

#[component]
fn CounterView(s: &Counter) -> View {
    <section class="counter" aria-label="Counter">
        <h1>"Hello, {s.name}!"</h1>

        <button on:click={|s, _| s.count -= s.step}>"−"</button>
        <output class:warn={s.count < 0}>"{s.count}"</output>
        <button on:click={|s, _| s.count += s.step}>"+"</button>

        <input bind:value={s.name} placeholder="Your name" />

        {if s.count > 10 {
            <p class="big">"That's a lot."</p>
        } else {
            <></>
        }}

        <ul>
            {for item in s.history.iter() by item.id {
                <li>"{item.label}"</li>
            }}
        </ul>
    </section>
}
```

### 3.2 Grammar decisions

| Construct | Syntax | Notes |
|---|---|---|
| Element | `<tag attrs…> children </tag>` / `<tag attrs… />` | Prefix-position `<` + ident (F5). |
| Fragment | `<> … </>` | |
| **Text** | **Always a string literal**: `"Hello"`, `"Hello {name}"` | No raw text mode ⇒ **no lexer change** (F4), no JSX whitespace-collapsing rules. Spaces between siblings are not significant; write `" "` explicitly (or use layout). |
| **Interpolated text** | `"Count: {s.count}"` | The GSX parser *splits the literal into static pieces + typed holes at parse time*, so each hole's read-set is syntactically visible and escaping is type-driven (G6). |
| Dynamic child | `{expr}` | Any expression of type `View` or a `Text`-able type. |
| Attribute | `name="lit"`, `name={expr}`, bare `disabled` (= `true`), shorthand `{name}` (= `name={name}`) | |
| Spread | `{..props}` | Typed against the target's prop struct. |
| Event | `on:click={handler}` | Handlers are **state transformers** `fn(&mut S, E)`, not capturing closures (§7.3). |
| Class/style toggles | `class:active={cond}`, `style:width={px}` | Statically decomposed into slots. |
| Two-way binding | `bind:value={place}` | `place` is a *place expression* (lvalue); see §7.5. |
| Control flow | native `if`/`match`/`for` inside `{ }` | **The language is the template language.** No `v-if`, no `{#each}`. The compiler recognizes these forms as *regions* (§6.3). |
| Keyed iteration | `for x in xs by <key-expr> { … }` | `by` is a GSX-scoped contextual keyword, accepted only in child-position `for`. |
| Component | `<Name …/>` (uppercase / path) | Resolved through the def-map like any item. |
| Host intrinsic | `<div/>` (lowercase) | Resolved through the active **dialect** schema (§5), never string-matched. |
| Namespaced dialect | `<web:canvas/>`, `<native:window/>` | Escape hatch for host-specific elements. |
| Escape hatch macro | `gsx! { … }` | Optional token-tree entry for use inside macros (F6). |

### 3.3 Why quoted text is a feature, not a compromise

- Zero lexer modes; tags, attrs, strings and braces all tokenize with the lexer you have.
- Whitespace semantics are explicit and identical across web and native (native layout has no "inline whitespace" concept anyway).
- Interpolation holes are lexically obvious → analyzable.
- A *later* opt-in JSX-style raw-text mode can be added with a parser-driven re-lex without breaking anything (§13).

### 3.4 CST nodes to add (`glyim-syntax`)

`GsxElement, GsxOpenTag, GsxCloseTag, GsxSelfClosing, GsxFragment, GsxAttr, GsxDirectiveAttr (ns:name), GsxSpread, GsxTextLit (string with holes), GsxBlockChild, GsxForBy`. Add `ast_node!` wrappers alongside the existing ones, and a `parse_gsx_element` arm in `parse_primary_expr` guarded by lookahead.

Parser gotchas to handle explicitly: the lexer's greedy `>>`, `>=` tokens next to tag closers (`<A b={x}>=`… split like the generics path code already does around `Lt`/`Gt` depth counting at `expr.rs:550/999`); and the existing `Ident Colon` label heuristic at the top of `parse_primary_expr`, which GSX directive names (`on:click`) must not trip.

---

## 4. The pipeline

### 4.1 Where GSX plugs in

```
 source.g
    │  Lexer (unchanged)                                    glyim-frontend
    ▼
 CST with Gsx* nodes  ◄── parse_gsx_element in parse_primary_expr
    │  macro expansion (gsx! escape hatch only)             glyim-meta
    ▼
 HIR: Expr::Gsx(GsxNode)   ← structure preserved, NOT desugared      glyim-hir
    │
    ▼
 Typeck: check_gsx                                           glyim-typeck
    │   • resolve <tag> → component def  |  dialect Schema
    │   • check props / events / children against the schema
    │   • infer the state type S and handler signatures
    ▼
 THIR: Gsx                                                   (typed)
    │
    ▼
 ┌──────────────────── NEW: glyim-gsx ─────────────────────┐
 │  1. Build VIR (View IR): skeleton + slots + regions      │
 │  2. Analyses: read-sets, write-sets, invalidation classes│
 │  3. Passes: hoist, specialize, inline, MSP generation    │
 │  4. Emit: Blueprint statics + MIR bodies                 │
 │        mount / patch_<W> / patch_mask / unmount          │
 │        render_to (SSR) / adopt (hydrate)                 │
 └──────────────────────────────────────────────────────────┘
    ▼
 MIR ─► Borrowck ─► glyim-opt ─► LLVM | bytecode VM
```

**Why a dedicated stage above MIR (from F9):** `glyim-opt` is per-body with no inliner or interprocedural analysis. Everything GSX needs (component inlining, call-graph write summaries, slot dependency maps) is *interprocedural and structural*. By the time it is MIR, the structure is gone. So VIR analysis must see THIR, and MIR optimizations only polish the output.

**Borrowck interplay:** user code (handlers, view functions, state) goes through borrowck normally. Compiler-generated `mount/patch/unmount` bodies are marked `#[generated]`, trusted by construction, and skip borrowck, but still pass `glyim_opt::validate_body`.

### 4.2 New crates

| Crate | Kind | Role |
|---|---|---|
| `glyim-gsx` | Rust (compiler) | VIR, analyses, passes, emission. Host-agnostic. |
| `glyim-lang-ui` | `.g` library (like `glyim-lang-core`) | `View`, `Host` traits, `Handler`, `Signal`, `Region` runtime, core dialect schemas. |
| `glyim-host-html` | `.g` lib | SSR string host (**first host to ship**). |
| `glyim-host-test` | `.g` lib | Headless op-recording host (**second**, powers perf tests, §11). |
| `glyim-host-native` | `.g` + FFI | Native widgets / retained scene (wgpu/winit or platform toolkit via `extern "C"`). |
| `glyim-host-web` | `.g` + tiny JS shim | DOM via command-buffer protocol (needs wasm, F17). |
| `glyim-host-tui` | `.g` | Terminal host: proves host-agnosticism cheaply. |

---

## 5. The core abstractions: Host, Dialect, Schema, View

### 5.1 Four concepts

| Concept | What it is | Who defines it |
|---|---|---|
| **Schema** | Machine-readable description of one element: props + types, events, content model, a11y role, invalidation class per prop, serialization form | Dialect author (data/comptime) |
| **Dialect** | A named set of schemas (`web`, `core`, `native`, `tui`) | Library |
| **Host** | The runtime that *realizes* operations (create/set/insert/remove/listen) | Library |
| **View** | The semantic value a component function returns; the compiler turns it into `mount`/`patch` code | Compiler |

The compiler passes only know **Schemas**. They never contain the string `"div"`. That is what keeps GSX "not tied to the web".

### 5.2 Schema example

```glyim
#[dialect(web)]
mod web {
    #[element(tag = "button", role = Button, content = Phrasing)]
    struct Button {
        #[affects(Paint)]            class: ClassList,
        #[affects(Structure, A11y)]  disabled: bool,
        #[affects(Layout)]           style_width: Length,
        #[event]                     click: Event<MouseEvent>,
        #[a11y(required_if(no_text_child))] aria_label: Option<Text>,
    }
}
```

Fields consumed by passes:

| Schema field | Consumed by |
|---|---|
| prop type | typeck (checking), escaper selection (G6) |
| `affects(...)` | invalidation-class pass (G5) |
| `content` (allowed children) | typeck children check, lint |
| `role`, `a11y(...)` | a11y lint (compile-time), a11y tree emission (G14) |
| `event` | handler typing, delegation table (G8) |
| serialization (void elements, boolean attrs) | SSR emitter (G7) |

Big dialects (full HTML5) should be **generated from data** (WHATWG-derived JSON) rather than hand-written, via `include!`-style data loading today and a `comptime fn` with an `fs` capability once the CVM exists (F2).

### 5.3 Portable "core" dialect + host dialects

```glyim
<Stack axis="vertical" gap={8}>
    <Text role="heading">"Settings"</Text>
    <Toggle label="Dark mode" bind:checked={s.dark} />
    <Button on:click={save}>"Save"</Button>
</Stack>
```

`core` elements (`Stack, Text, Image, Button, Toggle, Input, Scroll, List`) have a mapping per host: web → `div/span/button…`; native → platform widgets; tui → boxes. Host-specific elements live in their own namespace (`<web:canvas>`, `<native:menu_bar>`). A library written in `core` renders anywhere. This is the "not tied to the web" guarantee made concrete.

### 5.4 The Host trait

```glyim
// NOTE (F12): associated types depend on projection normalization, which has an
// open ICE. v0 may express Node/Event as generic params on Host instead.
trait Host {
    type Node;
    fn create(&mut self, tag: TagId) -> Self::Node;
    fn set_text(&mut self, n: &Self::Node, s: &str);
    fn set_prop<V: PropValue>(&mut self, n: &Self::Node, p: PropId, v: V);
    fn insert(&mut self, parent: &Self::Node, child: &Self::Node, before: Option<&Self::Node>);
    fn remove(&mut self, n: Self::Node);
    fn listen(&mut self, n: &Self::Node, ev: EventId, h: HandlerId);
    fn invalidate(&mut self, class: Invalidation);     // Paint | Layout | Structure | A11y
    fn instantiate(&mut self, bp: &'static Blueprint) -> Self::Subtree;  // bulk fast path
}
```

Three host *flavors*, expressed as a trait hierarchy so the compiler can drop code:

| Flavor | Examples | What the compiler emits |
|---|---|---|
| **Stream** (write-once) | SSR string, static export | `render_to` only. **No patch code at all.** |
| **Retained** | DOM, native widgets, TUI buffer | `mount`, `patch_*`, `unmount` |
| **Immediate / command** | GPU scene, canvas | `patch` regenerates a command list (still slot-skipped) |

Monomorphization over the host (Glyim already monomorphizes) gives a **zero-cost host abstraction**: the generated code calls `Host::set_text` directly and LLVM inlines it. No vtable, no vdom.

### 5.5 Views are programs-to-be-compiled, not values

In the common case, a view function is **never called at runtime**. It is the *source* from which `mount` and `patch_*` are derived. A `dyn View` fallback exists for genuinely dynamic composition (plugin components, user-supplied child closures) and is explicitly the slow path, with a lint that tells you when you fell into it.

---

## 6. The compilation model

### 6.1 Blueprint + slots

Every view is split into:

- **Blueprint**: the static skeleton, emitted as a flat `u32` op array plus string table in a `static` (F10). Ops: `Create(tag) / StaticProp(id, strid) / Text(strid) / Enter / Leave / SlotMarker(i)`.
- **Slots**: dynamic holes, each with a kind and a read-set:

| Slot kind | Example | Update action |
|---|---|---|
| `Text` | `"Count: {s.count}"` hole | `host.set_text` |
| `Prop` | `value={s.name}` | `host.set_prop` |
| `ClassToggle` | `class:warn={s.count<0}` | add/remove class bit |
| `Style` | `style:width={px}` | `set_prop` + `invalidate(Layout)` |
| `Event` | `on:click={…}` | one-time `listen` at mount (static handler id) |
| `Region` | `if / match / for / <Child/>` | structural, see 6.3 |

### 6.2 Mount

`mount` = `host.instantiate(&BLUEPRINT)` (one bulk call; DOM: template clone; native: prebuilt retained subtree; SSR: not applicable) followed by initial slot fills. For a fully static subtree there are **no slots, no patch code, and nothing is ever visited again**.

### 6.3 Regions

`if/else`, `match`, `for … by`, component calls and `{children}` become **Regions**: a dynamic structural point with an anchor node and N child blueprints.

- `if/match`: arm index stored; patch switches arms (unmount old, mount new). Arm bodies are themselves Blueprints with slots.
- `for … by key`: keyed list region, each row a sub-mount with *index-parameterized* patch functions (see §7.6, G11).
- Component call: **inlined by default** into the parent's mount/patch (G9); boundary preserved when recursive, large, or reused from many sites, via an inline heuristic using VIR node count (use `#[inline(never)]`/`#[inline(always)]` to override).

### 6.4 Output per component per host

| Artifact | Purpose |
|---|---|
| `BLUEPRINT` static | skeleton |
| `mount(host, &S, parent) -> Mounted` | build + initial fill |
| `patch_<W>(host, &S, &mut Mounted)` | one per distinct handler write-set (§7.5) |
| `patch_mask(host, &S, &mut Mounted, mask)` | fallback for dynamic entry points |
| `unmount` | arena free + host removal |
| `render_to(&mut W, &S)` | SSR (stream hosts) |
| `adopt(host, &S, existing)` | hydration of server-rendered tree, *only for non-static regions* (G19) |

---

## 7. The headline idea: ownership-driven reactivity

This is the idea I think is most distinctive *and* most natural for Glyim, because it uses machinery the compiler already has (field-granular `Place`s in MIR, F8; NLL borrow facts; move analysis).

### 7.1 The problem with existing models

| Model | Cost |
|---|---|
| React / vdom | Re-run view function, allocate tree, diff, every update. |
| Signals (Solid, Leptos, Dioxus) | Runtime dependency graph, subscription bookkeeping, allocation per signal; in Rust, `Rc<RefCell>`/`Copy` signal arenas to dodge the borrow checker. |
| Compose / Svelte | Compiler infers stability or dirty bits, but must trust or approximate aliasing, because the host language has no ownership proof. |

### 7.2 The Glyim approach

> **State is owned data. Views read it by shared borrow. Handlers mutate it by exclusive borrow. Since the borrow checker already proves that no one else can observe a mutation without going through an exclusive path, the set of places a handler can write is statically computable, and the set of places a slot reads is statically computable. Their intersection IS the change-detection system. No runtime tracking required.**

### 7.3 Handlers are state transformers, not capturing closures

`on:click={|s, e| s.count += 1}` has type `Handler<S, MouseEvent> = fn(&mut S, E)`. The compiler passes the component's state; the closure captures nothing mutable. This:

- sidesteps the notorious "closure captures `&mut state` while view borrows it" fight Rust UI frameworks have,
- makes every handler a *plain function with a MIR body* whose write-set is analyzable,
- is the Elm architecture, which scales well and is easy to test.

Handlers may also capture immutable `Copy`/`&T` data (e.g. loop index); these become inline env fields (G8).

### 7.4 Read-sets and write-sets

**Read-set of a slot:** the set of state *places* (field paths rooted at the component's state parameter, e.g. `s.count`, `s.user.name`) that the slot expression reads. Computed from THIR; calls into functions taking `&S` use an **interprocedural read summary** (fields they touch), conservatively the whole subtree at the argument place if the callee is opaque.

**Write-set of a handler (or any entry point):** places written, computed from the handler's MIR: `Assign(Place{local=s, projection=[Field…]})`, plus `&mut` reborrows passed to callees, resolved via **write summaries** computed bottom-up over the call graph (recursion → fixed point; extern/`dyn` → conservative "writes everything below this place").

**Interference** is prefix-based: a write to `s.user` interferes with a read of `s.user.name` and vice versa; `s.user.age` and `s.user.name` do not interfere. This is computed at compile time into constants.

### 7.5 Mutation-specialized patch functions (MSP): the killer optimization

For each distinct handler write-set `W`, the compiler generates:

```
patch_W(host, &S, &mut Mounted) {
    // ONLY the slots whose read-set intersects W, in tree order, straight-line.
    host.set_text(n3, fmt(s.count));
    host.set_prop(n5, CLASS, class_bits(s.count < 0));
}
```

and wires dispatch as `on click → handler(&mut s, e); patch_W(...)`. There is **no dirty flag, no diffing, no subscription table, no branching** on the fast path. Straight-line host calls. The `−` and `+` buttons above share `W = {s.count}`, so they share one patch function.

Fallbacks, all statically chosen:

| Situation | Strategy |
|---|---|
| Multiple events batched in one frame | OR constant masks, call `patch_mask` (a branch per *affected-bit slot group*, not per slot) |
| Entry point with opaque write-set (dyn callee, FFI) | `patch_all` or mask returned by callee |
| Writes via interior mutability / shared handles | explicit `Signal<T>` (7.7) |

### 7.6 Collections: index-parameterized write-sets

`s.items[i].done = true` has write-set `s.items[*].done` with *runtime* index `i`. Rows in a `for … by key` region are mounted with their index as a patch parameter, so the handler's patch becomes `patch_row_done(host, &S, &mut Mounted, i)`, which updates **only row `i`**. Structural writes (`push`, `remove`, `sort`) have write-set `s.items` (the container) and trigger the region's keyed reconcile (G11). This is the hardest part of the design (§13).

### 7.7 Where the static story ends: `Signal<T>`

If a slot reads through `Rc<RefCell<_>>`, `Cell`, a global, or anything the compiler can't track via exclusive paths, it's a **compile error with a fix-it**:

```
error[G0201]: slot reads `Cell<i32>`, which the compiler cannot track
  = help: wrap in `Signal<i32>`; writes will bump a bit in this component's dirty mask
```

`Signal<T>` writes set a named bit through the host scheduler. So there are two mechanisms in one mask space: **static place analysis for owned state (free), explicit signals for shared state (pay-as-you-go).** Nothing is implicit or magical, and the diagnostic tells you exactly why.

### 7.8 Composition = lens composition for free

A child component receives state as a **place**: `<Child state={s.child} />`. Because places are field paths, a child handler `fn(&mut Child, E)` composes with the parent as `fn(&mut Parent, E)` by *prefixing the place path*, and write-sets translate by prefixing as well. Props that are not state pass as `&T` reads. Rule of thumb: **props are reads, state is places.**

### 7.9 `bind:` via typed places

`bind:value={s.name}` desugars (at the VIR level) into a `Prop` slot reading `s.name` plus a synthesized handler `|s, e| s.name = e.value()`. The place type must equal the prop type, checked by typeck. The synthesized handler's write-set `{s.name}` yields an MSP patch that **suppresses the echo into the same input** (the compiler knows the DOM already holds that value), so there is no cursor-jump bug class.

---

## 8. Performance catalog

Tags: **[S]** standard / table-stakes, **[A]** adopt from prior art, **[B]** distinctive bet. Honest prior art in §14.

| ID | Idea | Mechanism | Tag |
|---|---|---|---|
| **G1** | **Blueprint in rodata + bulk instantiate** | Static skeleton as a `u32` op array; one `host.instantiate` call; DOM maps to template clone, native to prebuilt retained subtree | A |
| **G2** | **Static hoisting & folding** | Fully static subtrees have zero slots; adjacent static attrs merge; constant `class` strings concatenate at compile time | S |
| **G3** | **Place-level slot dependencies** | Read-sets/write-sets from THIR+MIR field projections (§7.4) | **B** |
| **G4** | **Mutation-specialized patch functions** | One straight-line patch per distinct write-set; zero runtime change-tracking on the fast path (§7.5) | **B** |
| **G5** | **Schema-driven invalidation classes** | Each prop declares `affects(None\|Paint\|Layout\|Structure\|A11y)`; a patch emits *one* `host.invalidate(max class)` at its end. Only-paint changes never trigger layout (native) / never touch layout-affecting props (web) | **B** |
| **G6** | **Type-driven, context-aware escaping** | Hole *position* (text / attr / URL / style / script) is known statically; escaper chosen at compile time; integers, bools and enums-with-identifier-variants need *no* escaping; `Text` vs `TrustedHtml` types make XSS unrepresentable without an `unsafe`/`trusted!` marker | **B** (types+compile-time) |
| **G7** | **SSR as const chunks** | Static HTML concatenated into `&'static str` chunks; output = memcpy chunk, escape-and-write hole, memcpy chunk; fully static pages are one slice with precomputed `Content-Length`; streaming flush points at `<Suspense>` boundaries | A |
| **G8** | **Handlers as static fns + inline env** | No per-node closure allocation; event delegation table is a `const` array indexed by `u16` handler id; one root listener on web | A |
| **G9** | **Prop specialization across component boundaries** | Components called with constant props are cloned and constant-propagated at VIR level; unused props eliminated per instantiation; extends the existing polymorphization idea (`glyim-lower/polymorphize.rs`) from generics to props. Needed because there is no MIR inliner (F9) | **B** |
| **G10** | **Arena-allocated mounted trees + drop elaboration** | Parent owns child; a region is one arena; unmount = one bulk free; no `Rc` cycles; sidesteps partial-move leaks (F13) | S |
| **G11** | **Statically chosen list strategy** | `by` key kind selects index / keyed / append-only / fixed; LIS reorder for keyed; row patch fns take the index (§7.6) | A |
| **G12** | **Layout partial evaluation (native)** | Static subtrees with constant constraints get layout computed at compile time; runtime relayout limited to dirty subtrees via G5 + slot read-sets | **B** |
| **G13** | **Whole-program style compilation** | Scoped styles → atomic classes (web) / `const` style tables (native); dead style elimination because the compiler sees every use | A |
| **G14** | **A11y tree emitted from schema** | Roles/labels/required props from schemas; a11y nodes built alongside the view; violations are compile errors | **B** |
| **G15** | **Unified command protocol** | The Blueprint op encoding *is* the wasm→JS command-buffer protocol; host ops batch into one contiguous buffer flushed once per frame; a ~1–2 KB JS interpreter handles both static instantiation and dynamic patches (minimizes costly wasm↔JS crossings) | A/B |
| **G16** | **Automatic partial hydration / islands** | Compiler knows which regions contain handlers or reactive slots; `adopt` is generated only for those; static regions are never walked | A |
| **G17** | **Dead code by host flavor** | Stream hosts compile *no* patch/unmount/event code; retained hosts compile no `render_to` | S |
| **G18** | **Bytecode-VM host for dev** | Same VIR → bytecode backend for instant rebuild and hot reload; LLVM for release (F15) | **B** (leverages unique asset) |

### 8.1 Expected cost model (what you can promise)

| Operation | Typical frameworks | GSX target |
|---|---|---|
| Update one text slot after handler | re-run view + diff, or signal graph walk | **1 host call**, straight-line |
| Static subtree update cost | diffed or walked | **0** (not visited) |
| Memory per static node | object + props map | **0 bytes** beyond host node (skeleton lives in rodata) |
| Event handler per node | closure allocation | **0 allocations** (static fn + inline env) |
| SSR for static content | string building | **memcpy of one slice** |

These are design targets, not measurements. §11.4 describes how the headless host lets you *assert* them in CI from day one.

---
