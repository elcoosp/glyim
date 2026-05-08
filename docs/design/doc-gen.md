Let's turn `glyim-doc` into the crown jewel of Glyim tooling—​a lightning‑fast, shadcn/ui‑styled, React‑powered static documentation site that rivals anything in the language ecosystem, with **zero JavaScript syntax highlighting** and complete control over styling. Here's the plan.

---

## 1. Architecture: Compiler‑Driven, Zero‑JS Highlighting

The **core tenet**: Syntect (Rust) generates highlighted HTML at build time, which React renders as static markup. JavaScript only handles interactive features that truly benefit from it (search, playground). Your compiler already holds all semantic information; we'll harness it to produce a rich JSON and pre‑rendered code blocks.

### Pipeline Overview

| Phase | Layer | Technology |
|-------|-------|------------|
| **Extraction & Highlighting** | Rust binary (`glyim-doc-syntect`) | Reads HIR, writes `api.json`, plus a directory of pre‑highlighted `.html` snippets for every code block. Uses `syntect` with a custom Glyim syntax definition. |
| **Static Site Generation** | Rspress + React 19 | MDX pages populated with React components (`shadcn/ui` + Tailwind v4) that read the JSON and inject the pre‑highlighted HTML. |
| **Build Orchestration** | `glyim doc` CLI | Calls the Rust binary, then executes `npx rspress build`. |

**Why this is blazingly fast:**  
- Syntect highlights all code at build time → zero runtime penalty.  
- Rspress uses Rspack (Rust‑based bundler) → builds your SSG in seconds.  
- Tailwind v4 with on‑demand JIT → tiny CSS.  
- Final output is pure HTML/CSS with minimal JS for interactive widgets.

---

## 2. Syntax Highlighting with Syntect (No JS)

### Create a small Rust crate inside your workspace: `glyim-doc-syntect`

```toml
[package]
name = "glyim-doc-syntect"
version = "0.1.0"
edition = "2024"

[dependencies]
syntect = "5.2"
glyim-hir = { path = "../glyim-hir" }
glyim-parse = { path = "../glyim-parse" }
serde_json = "1"
```

**What it does:**  
1. **Defines a Glyim TextMate grammar** (`.tmLanguage.json`). You write one from scratch using your existing `SyntaxKind` tokens—it's a JSON file describing token patterns, scopes, and colours. This gives you perfect highlighting control.  
2. **Processes every code block** found in doc comments and example files. For each block, it calls Syntect to produce an HTML string like `<span class="tok-kw">fn</span> <span class="tok-ident">main</span>() { ... }`.  
3. **Outputs the highlighted HTML** into a directory `docs/api/highlighted/` as individual snippet files, named by a content hash, along with a mapping file `highlight-map.json`.  

This binary runs **before** Rspress, so the highlighted HTML is available as static assets. The React components will simply insert it via `dangerouslySetInnerHTML` (you trust the source – your build pipeline). This gives you instant, perfect highlighting that matches your theme.

### Why not a WASM‑based solution in the browser?

You explicitly want performance and no JS. Generating thousands of highlighted lines ahead of time is the most efficient path. The output is cacheable and the browser doesn't need to parse syntax at all.

---

## 3. Frontend: Rspress + shadcn/ui + Tailwind v4

### Project Setup

```bash
npx create-rspress@latest docs
cd docs
npm install
npx shadcn@latest init
npx shadcn@latest add button card dialog tabs separator ...
npm install tailwindcss @tailwindcss/postcss --save-dev
```

Configure Rspress to use Tailwind (it works via PostCSS) and import shadcn/ui components. All styling uses shadcn's utility-first approach, which integrates seamlessly with Rspress’s theme.

**Why shadcn/ui?**  
- Components are copied into your project, giving full ownership and customization.  
- No unnecessary runtime – compiles down to minimal Tailwind classes.  
- Looks fantastic out of the box, with dark mode support via CSS variables.

### Directory Structure

```
docs/
  rspress.config.ts         – Rspress configuration
  src/
    components/             – React components (Signature, DocBlock, Playground, ...)
    lib/                    – utility functions, type definitions
    styles/                 – global Tailwind overrides
    pages/                  – Rspress pages (MDX or TSX)
    api/                    – generated JSON and highlighted snippets (git‑ignored)
  public/                   – static assets
```

---

## 4. The Ultimate Feature List

### 4.1 Signature Rendering with Deep Links

*   A `<Signature>` React component reads the type from `api.json` and renders it with proper formatting.  
*   Every type name is a clickable link to its own documentation page.  
*   Tooltips on hover show the full declaration, thanks to pre‑computed data.

### 4.2 Unified Search

*   **Algolia DocSearch** (or **FlexSearch** for offline) – search across all documented items with fuzzy matching.  
*   **⌘K command palette** (using shadcn's `CommandMenu`) – a quick‑access modal that lists functions, structs, and modules.

### 4.3 Code Previews & Playgrounds

*   **Pre‑rendered code blocks** with line numbers, highlighted lines (e.g., error spots), and copy‑to‑clipboard buttons.  
*   **Interactive playground** using Monaco Editor (lifted from shadcn's `CodeBlock` or custom) – users can type Glyim code and see compiled output.  
    - Option A: Compile Glyim via a **serverless function** (your Rust compiler compiled to WASI and run in a Cloudflare Worker).  
    - Option B: Compile in the browser using a **Glyim WASM module** (your codebase already compiles to Wasm; you can load it as a WebAssembly module).  
    - Option A/B are both feasible; choose based on complexity trade‑off.

### 4.4 Doc‑Test Integration

*   Every doc code example (````glyim```` fences) is automatically compiled and executed during the build step.  
*   The test result (pass/fail, exit code, stdout) is embedded as a badge next to the example.  
*   A filter can show/hide failing examples (important for maintaining documentation correctness).

### 4.5 AI‑Friendly Documentation

*   **llms.txt generation** – Rspress's `@rspress/plugin-llms` already does this.  
*   **Chat bot** – Embed an LLM that can answer questions about Glyim by referencing the docs (e.g., using a simple RAG setup with your site's content).

### 4.6 Versioned Docs

*   Rspress supports multi‑version through directory convention. Your `glyim doc` command accepts `--version` and outputs to a versioned path (`docs/v0.5.0/`). The site's header shows a dropdown to switch versions.

### 4.7 Macro Expansion Viewer

*   A custom `<MacroExpander>` component that shows the original code and the expanded version side‑by‑side, using a diff view powered by `diff2html` (or similar) to highlight changes.

### 4.8 LLVM IR Preview

*   When documenting a function, offer a collapsible section showing the generated LLVM IR (your codegen already produces it). This is a goldmine for learners and contributors.

### 4.9 Source Links & Breadcrumbs

*   Next to every signature, a link `[src]` that points to the exact line in your GitHub repository (using the span info from HIR).  
*   Breadcrumbs show the module path and the current item.

### 4.10 Dark/Light Mode & Custom Themes

*   shadcn/ui ships with a perfect dark mode via CSS variables. Tailwind v4 makes styling a breeze.  
*   Customise the colour palette to reflect Glyim's brand.  
*   Allow user‑selectable themes (e.g., "Glyim Light", "Glyim Night", "Solarized", "Monokai") that affect both the UI and the syntax highlighting colours (since Syntect can output class‑based styling, you can theme them with Tailwind classes).

---

## 5. Detailed Implementation Steps

### Step 1: The Rust Extraction & Highlighting Binary

**Extend `glyim-doc` to produce a unified JSON manifest.**  
The manifest contains a list of all public items, each with:
- Fully qualified name
- Kind (function, struct, enum, …)
- Canonical URL slug (e.g., `std.vec.push`)
- File‑based source location
- Rendered doc comment (HTML from pulldown‑cmark)
- Type signature (structured for the React `<Signature>`)
- List of code snippets found in the doc (each with its highlighted HTML content)

**Write the Glyim TextMate grammar** (JSON) that maps token scopes to your `SyntaxKind`. This is crucial for accurate highlighting. You can base it on the existing Glyim syntax and extend it for keywords, types, strings, etc.

**Implement `glyim-doc-syntect`** that:
1. Loads the `.tmLanguage.json` and a custom theme (exported as a CSS class map).  
2. Walks the manifest and, for each code block, call Syntect’s `highlighted_html_for_string`.  
3. Write the highlighted snippet to a file, store the file path in the manifest.

Result: `docs/api/api.json` and `docs/api/highlighted/*.html`.

### Step 2: Rspress Project Setup with shadcn/ui

Initialize Rspress as described. Import Tailwind and shadcn/ui components.

**Create React components:**
- `<Sidebar>` – dynamically generated from the manifest via Rspress’s `_meta.json` side‑effects (you can generate `_meta.json` from the manifest too).
- `<SignatureDisplay>` – takes a signature object and renders it as styled HTML with links.
- `<CodeBlock>` – loads the pre‑highlighted HTML from the static asset path and renders it with line numbers, copy button, and a “Run in playground” button.
- `<DocTestBadge>` – shows a green checkmark or red cross based on test results embedded in the manifest.
- `<MacroExpander>` – a tabbed pane that switches between original and expanded code.
- `<SearchCommandPalette>` – uses `cmdk` (or shadcn's `Command` component) to power the ⌘K menu.

### Step 3: MDX Page Generation

**Option A: Generate MDX files automatically from the manifest** using a script (which becomes part of `glyim doc`). For each item, produce a file like `docs/pages/struct.Point.mdx` with front‑matter:

```mdx
---
title: struct Point
slug: /std/struct.Point
---

import { SignatureDisplay } from '@/components/SignatureDisplay';
import { CodeBlock } from '@/components/CodeBlock';
import { DocTestBadge } from '@/components/DocTestBadge';

<SignatureDisplay item="std::struct::Point" />

<DocComment import="std::struct::Point" />

## Examples

<CodeBlock lang="glyim" snippet="hash123" />
<DocTestBadge status="pass" />
```

Each page imports data from a global context that reads `api.json`. This keeps pages DRY and the build fast.

**Option B (more efficient):** Use Rspress’s **`pages` directory with custom SSR** to dynamically render each route from the manifest without generating thousands of MDX files. You can create a catch‑all route `pages/doc/[slug].tsx` that reads the manifest and renders the appropriate component. This is cleaner and reduces build file count. I recommend this approach.

### Step 4: Interactive Playground

Set up a Glyim WASM module (your compiler already compiles to `wasm32-wasi`). Host it as a static asset and load it via a React component that wraps Monaco Editor. The component calls the WASM module’s `expand` or `compile` function and displays the output. This gives a fully in‑browser coding experience.

### Step 5: Search Integration

- For **Algolia**: use `@rspress/plugin-algolia` and provide an API key. The crawler can be triggered after deployment.  
- For **client‑side search**, use **FlexSearch** (already bundled in Rspress) or build a custom one with `fuse.js`. The manifest is small enough to be loaded entirely on the client for instant filtering.

### Step 6: Styling and Theming

Set up Tailwind v4 with the `@tailwindcss/postcss` plugin. Customise the shadcn theme via `tailwind.config.ts`. For syntax highlighting themes, export a Tailwind theme file that maps `.tok-*` classes to colours. This can be generated from your Syntect theme.

### Step 7: CI/CD & Deployment

Add a CI job:
```yaml
- name: Build Docs
  run: |
    cargo run -p glyim-doc -- --output-dir docs/api --version $VERSION
    cd docs && npm ci && npx rspress build
    # deploy to GitHub Pages or similar
```

---

## 6. Dependencies Summary

| Category | Library | Purpose |
|----------|---------|---------|
| Rust (backend) | `syntect` | Syntax highlighting (build time) |
| | `glyim-parse`, `glyim-hir`, `glyim-codegen-llvm` (for IR) | Compiler data |
| Frontend | `@rspress/core`, `react`, `react-dom` | SSG & framework |
| UI | `shadcn/ui` components, `tailwindcss` v4 | Styling & components |
| Playground | `@monaco-editor/react` | Code editor |
| Search | `@rspress/plugin-algolia`, `cmdk` | Search & command palette |
| AI | `@rspress/plugin-llms` | LLM‑friendly output |
| Code diff | `diff2html` | Macro expansion viewer |
| Clipboard | `react-copy-to-clipboard` | Copy button |
| Tooltips | `@radix-ui/react-tooltip` | Type hover tooltips (part of shadcn) |

---

## 7. Why This Approach Wins

*   **Build‑time highlighting** means the browser does zero work; the page is ready to paint immediately.  
*   **Rspress + shadcn/ui** gives a modern, responsive, and beautiful UI with minimal JS.  
*   **Full compiler integration** lets you show things no other doc generator can: macro expansions, LLVM IR, and live compilation/playground.  
*   **AI‑ready** out of the box — future‑proof for the agent workforce.

This plan transforms Glyim’s docs into a world‑class, interactive, and blazing‑fast resource. Every part of your existing codebase (from parsing to WASM compilation) feeds directly into a stunning developer experience.
Alright, you’ve set the stack and you’re not backing down—good. The harsh review is not a veto, it’s a filter. Let’s take every feature we talked about, run them through the critique’s impact/effort lens, and build a **realistic, sprint‑by‑sprint prioritization** that ships a dynamic, shadcn/ui‑powered, Rspress‑driven documentation site **that is actually awesome**, not just shiny.

---

## Glyim Docs 2.0 – Sprint Plan

### Guiding Principles After Critique

- **Examples must never lie** – The site will refuse to build if any doc‑test fails.  
- **Clickable everything** – Every type name, every function reference is a link.  
- **Search is the main navigation** – `⌘K` and full‑text search from day one.  
- **Dynamic doesn’t mean bloated** – Rspress, shadcn/ui, and Tailwind give you a polish/performance ratio that raw HTML can’t match. We’ll use them to solve real UX problems, not to show off.  
- **The hard stuff stays in Rust** – Syntax highlighting, type resolution, and doc‑testing remain build‑time operations so the browser never pays that cost.

---

### Sprint Zero: Foundation & CI (Week 1)

*Goal:* Prove the pipeline works end‑to‑end with a single dynamic page. No frontend flourishes yet.

| Task | Why it’s first |
|------|----------------|
| Extend `glyim-doc` to output `api.json` for a whole package (multiple `.g` files) | You can’t do anything else without data |
| Generate one MDX page per documented item (function, struct, enum) from that JSON | Demonstrates Rspress consuming compiler‑generated content |
| Add `glyim doc --test` that blocks on failure 🛑 | **This is the killer feature from the critique** – makes documentation trustworthy |
| Set up CI: build docs, run doc‑tests on every PR, fail if broken | Locks in quality immediately |

**Deliverable:** A live Rspress site with one module’s docs, all examples verified.

---

### Sprint 1: The Dynamic Experience (Week 2–3)

*Stack:* Rspress + shadcn/ui + Tailwind v4 activated. shadcn components scaffolded.

| Feature | Priority | Rationale |
|---------|----------|-----------|
| `<SignatureDisplay>` component – renders formatted signatures with **every type linked** to its own page | 🔴 Critical | Reviewer’s #1 request. Use `api.json` to resolve links. |
| shadcn `<Sidebar>` built from `_meta.json` (auto‑generated from module structure) | 🔴 Critical | Navigation that scales with your package |
| Pre‑rendered, **Syntect‑highlighted** code blocks with line numbers and copy buttons (keep it server‑side, no JS highlighting) | 🔴 Critical | Performance + “looks awesome” – the proof your “no JS” rule works |
| Full‑text search via native Rspress/FlexSearch + `⌘K` command palette (shadcn `CommandMenu`) | 🔴 Critical | Developers live in `⌘K`; instant, typo‑tolerant |
| Responsive dark/light mode via shadcn’s CSS variables | 🟡 High | table stakes for a modern site |

**Deliverable:** Polished site with browsing, linking, and search – already better than most lang docs.

---

### Sprint 2: Trust & Content Quality (Week 4)

| Feature | Priority | Rationale |
|---------|----------|-----------|
| **Doc‑test results embedded directly** next to each example (green check / red cross) + filter for failing examples | 🔴 Critical | The critique’s second killer feature – makes doc health visible |
| **Source `[src]` links** on every signature using HIR span data | 🟡 High | Builds trust, satisfies power users |
| **LLM‑ready output** (`llms.txt`, `llms-full.txt`) via Rspress plugin – free, just enable it | 🟡 High | The critique agreed this is useful and zero cost |
| **Versioned docs** – `glyim doc --version` stores output in versioned directory; Rspress config shows a switcher | 🟡 High | Addresses “which version am I reading?” complaint |

---

### Sprint 3: Deep Integration & Polish (Week 5–6)

| Feature | Priority | Rationale |
|---------|----------|-----------|
| **Interactive playground** using Monaco Editor + Glyim WASM (your `compile_to_wasm` already exists) | 🟡 High / Med | The critique said “expensive, but possible”. Start small: a single page with “Try it” button that opens a playground. Not embedded everywhere – that’s overkill. |
| **Macro expansion viewer** – side‑by‑side diff (not animated) of before/after macro expansion | 🟡 High | The critique acknowledged the static diff is useful. Animation can wait. |
| **Type tooltips on hover** – like Twoslash, but for Glyim – showing full type information from the HIR | 🟡 High | Leverages your compiler data for a magical dev experience |

---

### Sprint 4: The “Woah” Factor (Future, not in initial launch)

| Feature | Decision After Critique |
|---------|-------------------------|
| AI chatbot / embedded LLM search | **Defer.** The critique is right: adds latency, cost, and marginal benefit over full‑text search. Revisit only if users clamor for it. |
| Memory palace / knowledge graph navigation | **Kill.** No evidence this is useful; violates accessibility and established patterns. |
| Emoji safety scores | **Kill.** Compiler can’t reliably infer danger; doc comments already do this job. |
| Collaborative annotation layers | **Kill.** Requires a separate product; not appropriate for a doc generator. |
| Audio walkthroughs | **Kill.** No demand; OS screen readers already exist. |
| Hardware‑aware performance curves | **Defer until Glyim has benchmarking infra.** |

---

## Implementation Blueprint (Updated based on review)

### Stack

- **Backend:** `glyim-doc` crate (Rust) → `api.json` + pre‑highlighted snippets
- **Frontend:** Rspress (Rsbuild/React) + shadcn/ui + Tailwind v4
- **Syntax highlighting:** Syntect (Rust) at build time; no JS highlighting library
- **Search:** Rspress bundled FlexSearch + `cmdk` for `⌘K`
- **Playground:** Monaco Editor + Glyim WASM (loaded on demand)

### Rationale for Rejecting Raw HTML

The critique suggested “keep it simple, raw HTML first.” That under‑estimates the value of `⌘K` search, instant theme switching, tooltips, and interactive examples. Those are *not* just decoration – they fundamentally change how developers interact with documentation. Rspress gives you all of that scaffolding for free. Once you’ve written one `api.json`‑powered MDX page, you’ve proven out the dynamic pipeline. The `⌘K` bar alone justifies the shift from static files.

The key is to keep the heavy lifting in Rust (so the `api.json` stays lean), and let the React components be presentation‑only, not business logic. That way you get both performance and polish.

---

This plan ignores the fluff and focuses on features that either **prevent documentation from rotting** (doc‑test gating), **make navigation efficient** (`⌘K`, hyperlinked types), or **showcase Glyim’s unique strengths** (playground, macro viewer). The site will look phenomenal, load instantly, and earn developer trust. Exactly what you wanted.
I'm using the writing-plans skill to create the implementation plan.

**Plan complete and saved to `docs/superpowers/plans/2026-05-06-glyim-doc-2.md`. Ready to execute?**

---

# Glyim Doc 2.0: Dynamic, Test‑Gated Documentation Site – Full Sprint Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform `glyim-doc` into a full‑fledged documentation generator producing a dynamic, Rspress‑powered static site with shadcn/ui styling, pre‑highlighted Glyim code, hyperlinked type signatures, versioned docs, interactive playground, macro expansion viewer, and **CI‑enforced doc‑test gating** – examples that break block the build.

**Architecture:** The Rust crate `glyim-doc` becomes a compiler‑backed extraction engine. It walks the HIR, resolves types, extracts doc comments, highlights code with Syntect, and writes a structured JSON manifest + pre‑highlighted HTML snippets. The `docs/` directory (at repository root) is re‑initialised as an Rspress project using React 19, shadcn/ui, and Tailwind v4. React components read the manifest at build time to render dynamic, fast pages. Doc‑tests run in CI and the output site is deployed to GitHub Pages.

**Tech Stack:** Rust (`glyim-doc`, `glyim-compiler`, `syntect`, `serde`, `serde_json`, `walkdir`), Rspress (React 19 + Rsbuild), shadcn/ui, Tailwind CSS v4, TypeScript, `cmdk`, `@monaco-editor/react`, `Glyim WASM` for playground.

---

## Sprint Zero: Foundation & CI (Week 1)

### Goal
Prove the pipeline works end‑to‑end with a single dynamic page – manifest generation, highlighting, Rspress setup, basic components, and doc‑test gating.

### File Structure Changes

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/glyim-doc/Cargo.toml` | Modify | Add `syntect`, `serde`, `serde_json`, `walkdir` |
| `crates/glyim-doc/src/lib.rs` | Modify | Add new public modules, re‑export `generate_manifest`, `highlight_code` |
| `crates/glyim-doc/src/manifest.rs` | Create | `DocManifest`, `DocItem` structs |
| `crates/glyim-doc/src/generator.rs` | Create | `generate_manifest` function |
| `crates/glyim-doc/src/highlight.rs` | Create | Syntect integration |
| `crates/glyim-doc/src/syntaxes/Glyim.tmLanguage.json` | Create | Basic Glyim TextMate grammar |
| `crates/glyim-compiler/src/pipeline.rs` | Modify | Add `generate_doc_site` and `run_doc_tests` |
| `crates/glyim-cli/src/commands/cmd_doc.rs` | Modify | Accept package directory, `--test`, `--version` flags |
| `docs/` | **Delete** (backup if needed) | Will be replaced by Rspress scaffold |
| `docs/rspress.config.ts` | Create | Rspress configuration |
| `docs/package.json` | Create | Dependencies |
| `docs/src/components/SignatureDisplay.tsx` | Create | Signature with type links |
| `docs/src/components/DocBlock.tsx` | Create | Doc comment + highlighted code |
| `docs/src/components/DocTestBadge.tsx` | Create | Pass/fail badge |
| `docs/src/pages/doc/[slug].tsx` | Create | Dynamic doc page |
| `docs/src/lib/api.ts` | Create | Manifest loader |
| `.github/workflows/ci.yml` | Modify | Add doc‑test step |

---

### Task 0.1: Set up the Rust extraction engine

- [ ] **Step 1: Add dependencies to `glyim-doc`**

```toml
# crates/glyim-doc/Cargo.toml
[dependencies]
syntect = { version = "5", default-features = false, features = ["parsing", "html", "regex-onig"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
walkdir = "2"
```

- [ ] **Step 2: Create `DocManifest` and `DocItem` in `manifest.rs`**

```rust
// crates/glyim-doc/src/manifest.rs
use serde::Serialize;

#[derive(Serialize)]
pub struct DocManifest {
    pub package_name: String,
    pub version: String,
    pub items: Vec<DocItem>,
}

#[derive(Serialize)]
pub struct DocItem {
    pub kind: String,               // "fn", "struct", "enum", "impl", "extern"
    pub name: String,
    pub qualified_name: String,     // e.g., "io::Stdout"
    pub doc: Option<String>,       // cleaned Markdown
    pub signature_html: String,     // HTML with type links (initial simple formatting)
    pub source_file: String,
    pub source_line: u32,
    pub highlighted_examples: Vec<HighlightedExample>,
    pub doc_test_results: Vec<DocTestResult>,
    pub is_pub: bool,
}

#[derive(Serialize)]
pub struct HighlightedExample {
    pub code: String,
    pub html: String,              // Syntect output
    pub hash: String,              // SHA‑256 of code, used as filename for static file
}

#[derive(Serialize)]
pub struct DocTestResult {
    pub example_index: usize,
    pub passed: bool,
    pub output: String,
}
```

- [ ] **Step 3: Implement `generate_manifest` in `generator.rs`**

```rust
// crates/glyim-doc/src/generator.rs
use crate::manifest::*;
use glyim_compiler::pipeline::{PipelineConfig, compile_source_to_hir};
use glyim_hir::{HirItem, HirFn, StructDef, EnumDef, HirImplDef, ExternBlock};
use glyim_interner::Interner;
use glyim_pkg::manifest::load_manifest;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn generate_manifest(package_dir: &Path) -> Result<DocManifest, String> {
    // Load manifest
    let manifest_path = package_dir.join("glyim.toml");
    let manifest = load_manifest(&manifest_path).map_err(|e| format!("manifest: {e}"))?;
    let package_name = manifest.package.name;
    let version = manifest.package.version;

    // Discover source files
    let src_dir = package_dir.join("src");
    let mut file_paths = Vec::new();
    for entry in WalkDir::new(&src_dir).into_iter().filter_map(|e| e.ok()) {
        if entry.path().extension().map_or(false, |ext| ext == "g") {
            file_paths.push(entry.path().to_path_buf());
        }
    }

    let mut items = Vec::new();
    let config = PipelineConfig::default();

    for file_path in &file_paths {
        let source = std::fs::read_to_string(file_path).map_err(|e| format!("read {:?}: {e}", file_path))?;
        let compiled = compile_source_to_hir(source, file_path, &config)
            .map_err(|e| format!("compile {:?}: {e}", file_path))?;

        // Process each item in the monomorphized HIR
        let file_name = file_path.to_string_lossy().to_string();
        for hir_item in &compiled.mono_hir.items {
            if !is_item_public(hir_item) {
                continue;
            }
            let doc_item = hir_item_to_doc_item(
                hir_item,
                &compiled.interner,
                file_name.clone(),
                &package_name,
            )?;
            items.push(doc_item);
        }
    }

    Ok(DocManifest {
        package_name,
        version,
        items,
    })
}

fn is_item_public(item: &HirItem) -> bool {
    match item {
        HirItem::Fn(f) => f.is_pub,
        HirItem::Struct(s) => s.is_pub,
        HirItem::Enum(e) => e.is_pub,
        HirItem::Impl(i) => i.is_pub,
        HirItem::Extern(_) => true, // extern blocks are always public
    }
}

fn hir_item_to_doc_item(
    item: &HirItem,
    interner: &Interner,
    file_name: String,
    package_name: &str,
) -> Result<DocItem, String> {
    // Will be fleshed out in later steps
    // For now, return a placeholder
    Ok(DocItem {
        kind: "fn".into(),
        name: "placeholder".into(),
        qualified_name: format!("{}::placeholder", package_name),
        doc: Some("TODO".into()),
        signature_html: "<code>fn placeholder()</code>".into(),
        source_file: file_name,
        source_line: 0,
        highlighted_examples: vec![],
        doc_test_results: vec![],
        is_pub: true,
    })
}
```

- [ ] **Step 4: Write a unit test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;
    use std::io::Write;

    #[test]
    fn generate_manifest_basic() -> Result<(), String> {
        let dir = tempdir().unwrap();
        // Create minimal project
        fs::create_dir_all(dir.path().join("src"))?;
        fs::write(dir.path().join("glyim.toml"), r#"[package]
name = "test-pkg"
version = "0.1.0"
"#)?;
        fs::write(dir.path().join("src/main.g"), "pub fn hello() { 42 }")?;

        let manifest = generate_manifest(dir.path())?;
        assert!(manifest.package_name == "test-pkg");
        assert!(!manifest.items.is_empty());
        // Check that the item has kind "fn"
        assert_eq!(manifest.items[0].kind, "fn");
        Ok(())
    }
}
```

- [ ] **Step 5: Run test**

```bash
cargo test -p glyim-doc -- generator::tests::generate_manifest_basic
```

Expected: initial test will fail until `hir_item_to_doc_item` properly converts items. Implement `hir_item_to_doc_item` to extract the actual name, kind, doc string, etc.

---

### Task 0.2: Integrate Syntect highlighting

- [ ] **Step 1: Create a basic Glyim TextMate grammar**

Create `crates/glyim-doc/src/syntaxes/Glyim.tmLanguage.json`. Start with a minimal grammar:

```json
{
  "name": "Glyim",
  "scopeName": "source.glyim",
  "fileTypes": ["g"],
  "patterns": [
    { "include": "#keywords" },
    { "include": "#comments" },
    { "include": "#strings" },
    { "include": "#numbers" },
    { "include": "#identifiers" }
  ],
  "repository": {
    "keywords": {
      "match": "\\b(fn|struct|enum|impl|extern|let|if|else|return|match|pub|mut)\\b",
      "name": "keyword.control.glyim"
    },
    "comments": {
      "begin": "//",
      "end": "$\\n?",
      "name": "comment.line.double-slash.glyim"
    },
    "strings": {
      "begin": "\"",
      "end": "\"",
      "name": "string.quoted.double.glyim",
      "patterns": [
        { "match": "\\\\." }
      ]
    },
    "numbers": {
      "match": "\\b[0-9]+(\\.[0-9]+)?\\b",
      "name": "constant.numeric.glyim"
    },
    "identifiers": {
      "match": "[a-zA-Z_][a-zA-Z0-9_]*",
      "name": "variable.other.glyim"
    }
  }
}
```

- [ ] **Step 2: Write `highlight_code` and helper to load grammar**

```rust
// crates/glyim-doc/src/highlight.rs
use syntect::parsing::SyntaxSet;
use syntect::highlighting::ThemeSet;
use syntect::html::highlighted_html_for_string;
use syntect::util::LinesWithEndings;

lazy_static::lazy_static! {
    static ref SYNTAX_SET: SyntaxSet = {
        let mut ss = SyntaxSet::load_defaults_newlines();
        // Load our custom grammar from embedded bytes
        let bytes = include_bytes!("syntaxes/Glyim.tmLanguage.json");
        let grammar = syntect::parsing::SyntaxDefinition::load_from_str(
            std::str::from_utf8(bytes).unwrap(),
            true,
            Some("glyim"),
        ).expect("invalid grammar");
        ss.add_syntax(grammar);
        ss
    };
    static ref THEME_SET: ThemeSet = ThemeSet::load_defaults();
}

pub fn highlight_code(code: &str) -> String {
    let syntax = SYNTAX_SET.find_syntax_by_extension("g")
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let theme = &THEME_SET.themes["base16-ocean.dark"];
    match highlighted_html_for_string(code, &SYNTAX_SET, syntax, theme) {
        Ok(html) => html,
        Err(e) => {
            eprintln!("Highlighting error: {e}");
            // Fallback: HTML‑escape the code
            let escaped = html_escape::encode_text(code);
            format!("<pre>{}</pre>", escaped)
        }
    }
}
```

- [ ] **Step 3: Unit test**

```rust
#[test]
fn highlight_glyim_code() {
    let html = highlight_code("fn main() { 42 }");
    assert!(html.contains("<span class"));
    assert!(html.contains("42"));
}
```

- [ ] **Step 4: Integrate highlighting into `hir_item_to_doc_item`**

In `hir_item_to_doc_item`, for each doc comment, extract code blocks (reuse the existing `extract_code_blocks` from `glyim-doc/src/lib.rs`). For each block, highlight and store:

```rust
let highlighted = highlight_code(&code);
let hash = sha2::Sha256::digest(code.as_bytes());
let hex_hash = hex::encode(hash);
examples.push(HighlightedExample {
    code: code.clone(),
    html: highlighted,
    hash: hex_hash,
});
```

- [ ] **Step 5: Write static highlighted snippets to disk**

In `generate_manifest`, after building all items, create `docs/public/api/highlighted/` (or a temporary directory), and for each example, write the highlighted HTML to `{hex_hash}.html`. The path will later be used by the frontend.

---

### Task 0.3: Add CLI support for package dir and doc‑testing

- [ ] **Step 1: Extend `cmd_doc`**  

```rust
// crates/glyim-cli/src/commands/cmd_doc.rs
pub fn cmd_doc(input: PathBuf, output: Option<PathBuf>, open: bool, test: bool) -> i32 {
    let package_dir = if input.is_dir() {
        input
    } else {
        // backward compat: single file
        // existing behavior
        return cmd_doc_single_file(input, output, open, test);
    };

    let out_dir = output.unwrap_or_else(|| PathBuf::from("docs/public"));
    if test {
        match pipeline::generate_doc_site_with_tests(&package_dir, &out_dir) {
            Ok(has_failures) => {
                if has_failures {
                    eprintln!("Some doc-tests failed.");
                    1
                } else {
                    0
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        }
    } else {
        match pipeline::generate_doc_site(&package_dir, &out_dir) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        }
    }
}
```

- [ ] **Step 2: Implement `generate_doc_site` and `generate_doc_site_with_tests` in pipeline.rs**

```rust
// crates/glyim-compiler/src/pipeline.rs
pub fn generate_doc_site(package_dir: &Path, output_dir: &Path) -> Result<(), PipelineError> {
    let manifest = glyim_doc::generate_manifest(package_dir)
        .map_err(|e| PipelineError::Codegen(e))?;
    let api_dir = output_dir.join("api");
    std::fs::create_dir_all(&api_dir).map_err(PipelineError::Io)?;
    // Write JSON manifest
    let json = serde_json::to_string_pretty(&manifest).unwrap();
    std::fs::write(api_dir.join("api.json"), json).map_err(PipelineError::Io)?;
    // Write highlighted snippets
    let highlighted_dir = api_dir.join("highlighted");
    std::fs::create_dir_all(&highlighted_dir).map_err(PipelineError::Io)?;
    for item in &manifest.items {
        for example in &item.highlighted_examples {
            let path = highlighted_dir.join(format!("{}.html", example.hash));
            std::fs::write(path, &example.html).map_err(PipelineError::Io)?;
        }
    }
    Ok(())
}

pub fn generate_doc_site_with_tests(package_dir: &Path, output_dir: &Path) -> Result<bool, PipelineError> {
    // First, generate site without tests
    generate_doc_site(package_dir, output_dir)?;
    // Now run doc tests and update manifest with results
    let mut manifest = glyim_doc::generate_manifest(package_dir)
        .map_err(|e| PipelineError::Codegen(e))?;
    // Collect all code examples
    let mut failures = false;
    for (item_idx, item) in manifest.items.iter_mut().enumerate() {
        for (ex_idx, example) in item.highlighted_examples.iter().enumerate() {
            // Wrap in main, compile via JIT
            let source = format!("main = () => {{ {} }}", example.code);
            match run_jit(&source) {
                Ok(exit_code) => {
                    let passed = exit_code == 0;
                    item.doc_test_results.push(DocTestResult {
                        example_index: ex_idx,
                        passed,
                        output: format!("exit code: {}", exit_code),
                    });
                    if !passed { failures = true; }
                }
                Err(e) => {
                    item.doc_test_results.push(DocTestResult {
                        example_index: ex_idx,
                        passed: false,
                        output: format!("error: {e}"),
                    });
                    failures = true;
                }
            }
        }
    }
    // Write updated manifest with doc test results
    let api_dir = output_dir.join("api");
    let json = serde_json::to_string_pretty(&manifest).unwrap();
    std::fs::write(api_dir.join("api.json"), json).map_err(PipelineError::Io)?;
    Ok(failures)
}
```

- [ ] **Step 3: Add doc‑test step to CI**

Modify `.github/workflows/ci.yml`:

```yaml
      - name: Run doc-tests
        run: cargo run -p glyim-cli -- doc --test stdlib/
```

Now any broken doc example will fail the build.

---

### Task 0.4: Initialize Rspress project in `docs/`

> **Note:** Move existing `docs/` to `docs-archive/` if needed.

- [ ] **Step 1: Scaffold Rspress**

```bash
npx create-rspress@latest docs
cd docs
npm install
```

- [ ] **Step 2: Install shadcn/ui and Tailwind**

```bash
npx shadcn@latest init
npx shadcn@latest add button card dialog tabs separator command-menu tooltip
npm install tailwindcss @tailwindcss/postcss --save-dev
```

- [ ] **Step 3: Configure Tailwind**

Create `postcss.config.js` and adjust `rspress.config.ts`.

- [ ] **Step 4: Create the dynamic doc page**

See the earlier plan for `docs/src/pages/doc/[slug].tsx`. At this point, we can implement a simplified version that loads JSON from `/api/api.json` (it will be served because it's in `public/api/`).

- [ ] **Step 5: Create basic components**

Create `SignatureDisplay.tsx`, `DocBlock.tsx`, `DocTestBadge.tsx` as described earlier. For now, `SignatureDisplay` can just render the `signature_html` string as static HTML (not yet linking types). `DocBlock` loads the highlighted HTML from the static file.

- [ ] **Step 6: Verify the pipeline**

Run:

```bash
cargo build -p glyim-cli
./target/debug/glyim doc stdlib/ --output docs/public
cd docs && npm run dev
```

Visit `http://localhost:3000/doc/...` for some public items. Ensure no errors.

- [ ] **Step 7: Commit all changes**

---

## Sprint 1: Dynamic Experience (Week 2–3)

### Goal
Polish the UI: implement clickable type links, `⌘K` search, dark/light mode, full‑text search, mobile‑friendly layout.

### Additional Files

- `docs/src/components/ThemeToggle.tsx`
- `docs/src/styles/code-theme.css` (generated from Syntect theme)

### Task 1.1: Hyperlinked type signatures

- [ ] **Step 1: Extend `hir_item_to_doc_item` to generate linked HTML**

In the Rust generator, when building the signature string, replace type names with `<a class="type-link" href="/doc/{qualified_name}">TypeName</a>`. Build a lookup table of all public item qualified names. Then for each type used in the signature, if it matches a known item, produce a link.

Add a helper function `linkify_signature(signature: &str, all_items: &[DocItem]) -> String` that uses regex to find identifiers and wrap them if they exist in the item list.

- [ ] **Step 2: Re‑generate the manifest and verify links**

- [ ] **Step 3: Style the links in Tailwind**

---

### Task 1.2: Implement `⌘K` search with `cmdk`

- [ ] **Step 1: Create `SearchCommand.tsx`**

Use shadcn/ui's `Command` component. Load all items from `api.json` and enable fuzzy filtering.

- [ ] **Step 2: Integrate into layout**

Add a search button in the header that opens the command palette.

- [ ] **Step 3: Add keyboard shortcut**

---

### Task 1.3: Dark/Light mode

- [ ] **Step 1: Add a theme toggle component** using shadcn's dropdown.

- [ ] **Step 2: Configure Tailwind dark mode**

- [ ] **Step 3: Adjust syntax highlighting theme**  

Syntect produces classes like `.syntect .keyword`. Define CSS variables for these classes and switch them based on the theme.

---

## Sprint 2: Trust & Content Quality (Week 4)

### Goal
Embed doc‑test results, source `[src]` links, versioned docs, LLM‑ready output.

### Task 2.1: Doc‑test badges

- [ ] **Step 1: In `DocBlock.tsx`, display a badge next to each example**

If `doc_test_results` contains a result for this example index, show a green checkmark or red cross.

- [ ] **Step 2: Add a filter toggle to show only failing examples**

---

### Task 2.2: Source links

- [ ] **Step 1: In the Rust generator, add a field `source_url`** with a GitHub blob URL (based on version).  

In `DocItem`, include `source_url: String`.

- [ ] **Step 2: Render a `[src]` link in the signature area**

---

### Task 2.3: Versioned output

- [ ] **Step 1: Extend CLI to accept `--version`** and set output path accordingly.

- [ ] **Step 2: Configure Rspress `themeConfig.versions`** to show a version selector.

---

### Task 2.4: LLM‑friendly output

- [ ] **Step 1: Enable `@rspress/plugin-llms`** in `rspress.config.ts`.

---

## Sprint 3: Deep Integration & Polish (Week 5–6)

### Goal
Interactive playground, macro expansion viewer, type tooltips on hover.

### Task 3.1: Interactive playground

- [ ] **Step 1: Build Glyim WASM module**  

Modify `glyim-codegen-llvm` to compile the entire compiler pipeline to a WASM blob that takes source code and returns an exit code and any output. Store `glyim_wasm.wasm` in `docs/public/wasm/`.

- [ ] **Step 2: Create `Playground.tsx`**  

Use `@monaco-editor/react` and the WASM module. Add a “Run” button that compiles and shows the output.

- [ ] **Step 3: Add a “Try it in Playground” link** next to each code example that opens the playground pre‑filled.

---

### Task 3.2: Macro expansion viewer

- [ ] **Step 1: In the Rust doc generator, for any item that originated from a macro, capture the pre‑ and post‑expansion source** and store it in `DocItem`.

- [ ] **Step 2: Create a component `MacroExpander.tsx`** that renders a side‑by‑side diff using `diff2html`.

---

### Task 3.3: Type tooltips

- [ ] **Step 1: Extend the JSON manifest to include a `type_info` map** for each item, listing its type if it's a function, struct fields, etc.

- [ ] **Step 2: In `SignatureDisplay`, when hovering a type name, show a tooltip with the type's own documentation signature** (like Twoslash). Use `@radix-ui/react-tooltip`.

---

## Sprint 4: The “Woah” Factor (Future)

### Stretch features

- **LLM chatbot** – Defer unless community demands it.
- **Memory palace visualization** – Kill.
- **Emoji safety scores** – Kill.
- **Collaborative annotations** – Kill.
- **Audio walkthroughs** – Kill.
- **Performance curves** – Defer until Glyim has benchmarking infrastructure.

---

## Execution Handoff

Start with Sprint Zero tasks. After completing each task, commit with a message like `feat(doc): ...`. After finishing a sprint, deploy the site to a staging environment and gather feedback.

**Plan complete and saved to `docs/superpowers/plans/2026-05-06-glyim-doc-2.md`.** Ready to execute?
