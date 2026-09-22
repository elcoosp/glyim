# Glyim Agent Dispatch Workflow

This document explains how to use the **agent kit** to parallelize Glyim compiler development. You create **five files once**, then generate a brief for each stream and dispatch it to a subagent.

## The Five Core Files

| File | Purpose | Location |
|------|---------|----------|
| `AGENT_MASTER_CONTEXT.md` | Project‑wide rules (architecture, contracts, tracing, error handling) – included in every system prompt. | `docs/agent-kit/` |
| `CONTRACTS_LOCKED.md` | Auto‑generated list of all `pub` items in the codebase. Agents may NOT modify these. | `docs/agent-kit/` |
| `stream-template.md` | Markdown template for a stream brief. | `docs/agent-kit/` |
| `streams.json` | Single source of truth for all streams (id, name, crate, owned modules, locked interfaces, tests, upstream/downstream). | `docs/agent-kit/` |
| `generate-stream.sh` | Bash script that reads `streams.json` + template and outputs `briefs/S{XX}.md`. | `docs/agent-kit/` |

All five files are **committed to the repository**. Once they exist, you never write a stream brief manually again.

## Step 1 – Populate the Five Files

Copy the exact contents from the “Glyim Agent Dispatch Kit — Templates & Document System” section into your local `docs/agent-kit/` directory:

- `AGENT_MASTER_CONTEXT.md`
- `CONTRACTS_LOCKED.md`
- `stream-template.md`
- `streams.json`
- `generate-stream.sh` (make executable: `chmod +x generate-stream.sh`)

## Step 2 – Generate All Stream Briefs

```bash
cd docs/agent-kit
mkdir -p briefs

for id in S01 S02 S03 S04 S05 S06 S07 S08 S09 S10 \
         S11 S12 S13 S14 S15 S16 S17 S18 S19 S20; do
    ./generate-stream.sh "$id"
done
```

This creates `briefs/S01.md`, `briefs/S02.md`, … `briefs/S20.md`.

For **unstubbing streams** (U01…U08) you can either add them to `streams.json` and run the generator again, or manually create the briefs using the same template. The U‑streams defined earlier (U01‑U08) can be added to `streams.json` under a separate wave.

## Step 3 – Dispatch a Stream to an Agent

Each agent receives **exactly four documents** in their prompt:

| Document | Where to paste |
|----------|----------------|
| `AGENT_MASTER_CONTEXT.md` | System / developer message |
| `CONTRACTS_LOCKED.md` | System / developer message |
| `briefs/S{XX}.md` | User message |
| Source code of the owned crate(s) | User message (attached) |

### Example for Stream S01 (Lexer)

**System prompt (paste as one message):**

```text
<contents of AGENT_MASTER_CONTEXT.md>

--- 

<contents of CONTRACTS_LOCKED.md>
```

**User prompt (paste as a separate message):**

```text
You are implementing Stream S01: Lexer for the Glyim compiler.

## Your Stream Brief

<contents of briefs/S01.md>

## Source Code Context

### crates/glyim-frontend/src/lexer.rs

```rust
<full content of lexer.rs>
```

### crates/glyim-syntax/src/lib.rs

```rust
<full content of syntax/lib.rs>
```

### crates/glyim-test/src/lib.rs

```rust
<full content of test/lib.rs>
```

Follow the brief exactly. Write all tests first, then implement. Output complete modified files.
```

The agent will then produce **bash scripts** that apply the changes (using `cat` heredocs, `patch`, or direct file edits). You execute those scripts in order.

## Step 4 – Handling Dependencies

If a stream has `upstream` dependencies (e.g., S09 depends on S01), you **must complete the upstream stream first** and merge its PR before dispatching the downstream one. The brief automatically lists upstream streams – the agent will know they are already merged.

## Step 5 – Merging the Result

After the agent outputs the bash scripts:

1. Run each script in order.
2. Verify with `cargo test -p <crate>` and `cargo clippy -p <crate>`.
3. Create a PR against `main` with branch `stream-{XX}/v0.1.0`.
4. Once merged, move to the next stream.

## Adding New Streams (e.g., Unstubbing U01‑U08)

Edit `streams.json` and append the new stream objects (using the same schema). Then run `./generate-stream.sh U01` etc. The process is identical.

## Why This Works

- **Locked interfaces** prevent agents from touching unrelated `pub` items, avoiding merge hell.
- **All stubs are visible** – agents must replace every `STUB` warning with real code.
- **Test‑first** ensures every feature is validated before implementation begins.
- **Parallelism** – streams with no `upstream` dependencies can run simultaneously.

## Full List of Current Streams (v0.1.0)

| ID | Name | Crate | Wave |
|----|------|-------|------|
| S01 | Lexer | glyim-frontend | 1 |
| S02 | TypeInterning | glyim-type | 1 |
| S03 | MIRCore | glyim-mir | 1 |
| S04 | LayoutEngine | glyim-layout | 1 |
| S05 | Unification | glyim-solve | 1 |
| S06 | MIRInterpreter | glyim-mir-interp | 1 |
| S07 | BytecodeBackend | glyim-codegen | 1 |
| S08 | LLVMBackend | glyim-codegen-llvm | 1 |
| S09 | Parser | glyim-frontend | 2 |
| S10 | TypeDisplay | glyim-type | 2 |
| S11 | TraitSolver | glyim-solve | 2 |
| S12 | HIRLowering | glyim-hir | 2 |
| S13 | DefMap | glyim-def-map | 3 |
| S14 | TypeckDriver | glyim-typeck | 3 |
| S15 | MIRLowering | glyim-lower | 3 |
| S16 | Borrowck | glyim-borrowck | 3 |
| S17 | MIROpt | glyim-opt | 3 |
| S18 | Pipeline | glyim-pipeline | 4 |
| S19 | LSP | glyim-lsp | 4 |
| S20 | CLI | glyim-cli | 4 |

For the **unstubbing phase** (U01‑U08), add them to `streams.json` with wave 0 and no upstream dependencies where appropriate, then generate briefs and dispatch in parallel.

---

**Now you are ready to dispatch agents.** Every stream will be implemented in isolation, tested independently, and merged cleanly.
