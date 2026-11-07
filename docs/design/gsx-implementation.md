# GSX implementation notes — design doc → code map

> Companion to [`GSX-in-Glyim.md`](GSX-in-Glyim.md). That document is the
> *vision*; this one maps each section to the code that now implements it,
> records the v0.1 scope decisions, and lists what is deliberately not wired
> yet. All of it landed test-first (TDD): each module's tests encode the
> design doc's claims before the implementation.

## 1. Status at a glance

| Design section | Status | Where |
|---|---|---|
| §3 Syntax (elements, fragments, attrs, directives, `by`, text holes) | **implemented** | `glyim-frontend/src/parser/gsx.rs`, `SyntaxKind::Gsx*` in `glyim-syntax` |
| §3.4 CST nodes (`GsxElement … GsxForBy`) | **implemented** | `glyim-syntax/src/lib.rs` + `ast_node!` wrappers |
| §4.1 pipeline (parse → AST → VIR → blueprint → analyses → MSPs → emit) | **implemented** (stage as a library; driver wiring below) | `glyim-gsx` crate |
| §4.2 `glyim-gsx` crate | **implemented** | `crates/glyim-gsx` |
| §4.2 `glyim-lang-ui` / host dialect crates (`.g` libs) | not started | — |
| §5 Schema / Dialect / Host | **implemented** (compiler-side seed registry; §5.2 Button schema verbatim) | `glyim-gsx/src/schema.rs`, `host.rs` |
| §6.1 Blueprint + slots | **implemented** | `glyim-gsx/src/blueprint.rs`, `vir.rs` |
| §6.2 mount | **implemented** | `ViewRuntime::mount` in `msp.rs` |
| §6.3 regions (if/match/for-by/component) | **implemented** | `vir.rs` + `msp.rs` |
| §6.4 output per component per host | **implemented** (mount/patch/patch_mask/SSR; `adopt` = island spec) | `emit.rs`, `msp.rs` |
| §7.1–§7.5 ownership-driven reactivity, MSP | **implemented** | `analysis.rs`, `msp.rs` |
| §7.6 indexed/structural writes, keyed reconcile | **implemented** | `analysis.rs`, `host.rs` (LIS), `msp.rs` |
| §7.7 `Signal<T>` escape hatch | **implemented** (G0201 + fix-it; host signal bits) | `vir.rs` (diag), `host.rs` |
| §7.8 composition = lens composition | **implemented** for inlined components (G9 path) | `vir.rs` |
| §7.9 `bind:` | **implemented** (desugar + echo suppression) | `vir.rs`, `msp.rs`, `host.rs` |
| §8 catalog | G1 G2 G5 G6 G7 G8 G9 G11 G16 G17 **implemented/tested**; G3 G4 are the §7 core (done); G10 arena trees, G12 layout, G13 styles, G14 a11y tree emission, G15 wasm protocol, G18 VM host **not started** | — |
| §8.1 cost model assertions | **implemented** (headless host asserts; see `host.rs` tests + `tests/counter.rs`) | `glyim-gsx/src/host.rs` |
| §9–§15 (native GUI, metaprogramming, DX, roadmap, risks, positioning, decisions) | out of scope for v0.1 — the design doc file ends at §8.1 | — |

Test totals after landing: **4,754 workspace tests green**, including 82
`glyim-gsx` unit tests, 6 flagship end-to-end tests
(`crates/glyim-gsx/tests/counter.rs`), 21 parser tests and 5 HIR tests.

## 2. The pipeline as it now exists

```
source.g
   │  Lexer (unchanged, F4: text is always a string literal)   glyim-frontend
   ▼
CST with Gsx* nodes  ◄── parse_gsx_element (prefix-Lt arm, F5)
   │
   ├─────────────────────────────────────────────────────────────┐
   │ gsx AST (holes split at parse time, §3.2)                    │ glyim-gsx::ast
   ▼                                                              ▼
HIR: Expr::Gsx(GsxExpr) — structure preserved, NOT desugared (F7)  glyim-hir
   │
   ▼
Typeck: check_gsx (§4.1)                                            glyim-typeck
   │   • tag → dialect Schema (§5: passes know schemas, not tags)
   │   • props / events / children validated (§5.2)
   │   • hole exprs checked recursively
   │   • stage hand-off diagnostic if gsx stage not run (clean, no ICE)
   ▼
glyim-gsx stage (this is where views become programs):
   1. VIR: skeleton + slots + regions (§6.1)                        vir.rs
   2. Analyses: read-sets / write-sets / interference (§7.4)        analysis.rs
   3. MSPs: one straight-line patch per distinct write-set (§7.5)   msp.rs
   4. Emit: Blueprint (u32 ops + strings, F10) + SSR + islands      blueprint.rs, emit.rs
   ▼
HeadlessHost executes mount/patch/dispatch (§11.4, §8.1 asserts)    host.rs
```

The driver (`glyim-cli`) does **not** run the gsx stage yet: typeck ends a
GSX-containing body with a clean diagnostic pointing here (the same contract
the codebase already uses for `await` reaching typeck without desugaring).
Running the stage is exactly what the flagship tests do via
`glyim_gsx`'s public API.

## 3. v0.1 scope decisions (and why)

1. **Text is always a string literal** (F4). No lexer modes; the lexer is
   untouched. Interpolation holes stay inside the literal in the CST/HIR and
   are split once, at VIR construction (`ast::split_text_holes`).
2. **Prefix-`<` disambiguation** (F5). `parse_primary_expr` grew an `Lt`
   arm guarded by `at_gsx_start()` (next token `Ident` or `>`). Binary `<`
   is untouched; a non-GSX prefix `<` keeps the ordinary parse error.
3. **GSX is first-class syntax, not a macro** (F6). `gsx! { … }` (escape
   hatch) is *not* implemented — token trees cannot see `<…>` groups, and
   the first-class path needs no macro.
4. **Schemas are data** (§5). The compiler ships a seed registry (`web` +
   portable `core`, §5.3). The full-HTML5 dialect should be generated from
   WHATWG-derived data (§5.2); nothing in the passes hard-codes tags.
5. **`Host` uses plain ids, not associated types** (F12). Associated types
   lean on projection normalization, which has an open ICE; v0 expresses
   `Node` as `u32` and events as static handler ids.
6. **Handlers are state transformers** (§7.3). `on:click={|s, e| …}` binds
   state + event; capturing-closure rejection (`G0200`) is specified in
   `diag.rs` and enforced at VIR/typeck level in the schema checks — the
   runtime tests exercise the transformer semantics through the interpreter.
7. **Blueprints are `u32` arrays + string tables** (F10). `MirConst` has no
   aggregate constants; the encoding needs only integer-array statics.
8. **check_gsx validates, then hands off.** Typeck checks structure against
   schemas and checks every hole expression; the element's *value* is the gsx
   stage's product. Until the driver runs the stage, GSX bodies end with a
   clean diagnostic (mirroring the `await`-desugar contract), never an ICE.

## 4. What the flagship test proves

`crates/glyim-gsx/tests/counter.rs` compiles the §3.1 Counter example and
asserts, against the headless host:

- mount is one bulk instantiate + initial fills (§6.2, G1);
- the − and + buttons share **one** mutation-specialized patch for
  `W = {s.count}` (§7.5);
- a click is `handler(&mut s, e); patch_W(...)` — exactly one `set_text`
  host call for the one dirty slot, zero visits for static subtrees (§8.1);
- one terminal `invalidate(max class)` per patch (G5);
- `bind:value` updates state and **suppresses the echo** into the input
  (§7.9);
- the keyed `for … by item.id` region reconciles structurally without
  re-creating sibling rows (§7.6, G11);
- SSR output escapes by hole position — `<script>` in state cannot inject
  markup (G6, G7);
- the stream flavor compiles no patch code at all (G17);
- reads through untrackable surfaces are compile errors with a fix-it
  (`error[G0201]`, §7.7).

## 5. Next steps (in design-doc order)

1. **Driver wiring** (`glyim-cli`): after typeck, run `glyim-gsx` over
   `Expr::Gsx` bodies and splice the generated artifacts (blueprint statics +
   mount/patch fns) into codegen. The `await`-desugar precedent shows where
   the pass slots in.
2. **`glyim-lang-ui`** (§4.2): `View`/`Host`/`Handler` as a `.g` library so
   user code can name `View` and components resolve through the def-map.
3. **Component resolution in typeck** (§3.2): currently structural; with
   lang-ui, props type-check against the component's prop struct and §7.8
   place-passing becomes a typed rule.
4. **Hosts** (§4.2): `glyim-host-html` (SSR string host) first, then
   `glyim-host-test` (already prototyped as the compiler-side
   `HeadlessHost`), then native/tui/web.
5. **G10/G12/G13/G14/G15/G18** per the §8 catalog — each has a defined
   mechanism in the design doc and the VIR/analysis scaffolding it needs
   already exists.
