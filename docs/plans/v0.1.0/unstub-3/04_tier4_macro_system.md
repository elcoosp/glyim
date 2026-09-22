## TIER 4 — Macro system (`glyim-meta`)

### 4.1 Fragment-spec matching accepts anything — `glyim-meta/src/expander/matcher.rs`

**Current (line 187-206, confirmed):**
```rust
fn matches_fragment_spec(tree: &TokenTree, spec: &FragmentSpec) -> bool {
    match spec {
        FragmentSpec::Ident => matches!(tree, TokenTree::Token(SyntaxKind::Ident, _)),
        FragmentSpec::Literal => matches!(tree, TokenTree::Token(SyntaxKind::IntLit | ... , _)),
        FragmentSpec::Lifetime => matches!(tree, TokenTree::Token(SyntaxKind::Lifetime, _)),
        _ => true, // Expr, Ty, Path, Block, Stmt, Item, Pat, Vis, Meta, Tt all accept anything
    }
}
```

**Root architectural issue (confirmed, not in the report but necessary to
know before touching this):** `matches_fragment_spec` is called per **single**
`TokenTree`, but a real `$e:expr` fragment can span *many* token trees
(`a + b * c` is one `expr` fragment made of 5 token trees). The current
function signature literally cannot express "this fragment consumes 5
tokens" — it only answers yes/no for one. So this is not a one-line fix;
it's two staged fixes.

**Stage A (do this first — cheap, immediately reduces false-accepts):**
tighten the single-token cases that a fragment could plausibly still be
validated against without full parsing, and make everything else *reject*
patently-invalid single-token starts instead of blanket-accepting:
```rust
fn matches_fragment_spec(tree: &TokenTree, spec: &FragmentSpec) -> bool {
    match spec {
        FragmentSpec::Ident => matches!(tree, TokenTree::Token(SyntaxKind::Ident, _)),
        FragmentSpec::Literal => matches!(tree, TokenTree::Token(SyntaxKind::IntLit | SyntaxKind::FloatLit | SyntaxKind::StringLit | SyntaxKind::BoolLit | SyntaxKind::CharLit, _)),
        FragmentSpec::Lifetime => matches!(tree, TokenTree::Token(SyntaxKind::Lifetime, _)),
        FragmentSpec::Vis => matches!(tree, TokenTree::Token(SyntaxKind::KwPub, _)) || true, // `vis` is legitimately often empty/zero-width — see Stage B note
        FragmentSpec::Block => matches!(tree, TokenTree::Group(SyntaxKind::LBrace, _, SyntaxKind::RBrace)),
        FragmentSpec::Tt => true, // `tt` is DEFINED as "exactly one token tree" — `true` here is correct, not a placeholder
        FragmentSpec::Expr | FragmentSpec::Ty | FragmentSpec::Path | FragmentSpec::Pat => {
            // Reject tokens that can never start this fragment kind, even
            // though we can't yet confirm the whole fragment is valid.
            !matches!(tree, TokenTree::Token(
                SyntaxKind::Semicolon | SyntaxKind::Comma | SyntaxKind::RParen
                | SyntaxKind::RBrace | SyntaxKind::RBracket, _))
        }
        FragmentSpec::Stmt | FragmentSpec::Item | FragmentSpec::Meta => true, // see Stage B
    }
}
```
Confirm the exact `SyntaxKind` variant names (`KwPub`, `LBrace`/`RBrace`,
`Semicolon`, etc.) against `glyim-syntax/src/lib.rs` before pasting — this
plan used plausible names, not verified ones (this is the one place in
this tier worth double-checking against the enum directly since a typo
here fails to compile, unlike a logic bug).

**Stage B (the real fix — variable-length fragment consumption):** this
must live in `match_pieces` (line ~217), not `matches_fragment_spec`, since
only the caller sees the whole remaining `input: &[TokenTree]` slice and
can decide how many trees to consume. Add a sibling function:
```rust
/// Attempts to consume a maximal valid fragment of `spec`'s kind starting
/// at `input[pos]`. Returns the number of token trees consumed, or `None`
/// if no valid fragment starts there.
fn consume_fragment(spec: &FragmentSpec, input: &[TokenTree], pos: usize) -> Option<usize> {
    match spec {
        // Single-tree specs: unchanged, delegate to matches_fragment_spec.
        FragmentSpec::Ident | FragmentSpec::Literal | FragmentSpec::Lifetime
        | FragmentSpec::Tt | FragmentSpec::Block => {
            input.get(pos).filter(|t| matches_fragment_spec(t, spec)).map(|_| 1)
        }
        FragmentSpec::Expr | FragmentSpec::Ty | FragmentSpec::Pat
        | FragmentSpec::Path | FragmentSpec::Stmt | FragmentSpec::Item => {
            // Re-serialize input[pos..] back to source text (TokenTree already
            // carries original text per-token via `.text()`) and hand it to
            // the real frontend parser's fragment-specific entry point.
            let text = crate::expander::token_tree::retokenize_to_source(&input[pos..]); // NEW helper: join .text() with single spaces, reconstructing delimiters for Group trees
            let consumed_tokens = glyim_frontend::try_parse_fragment(spec.into(), &text)?; // NEW frontend entry point, see below — returns Some(N) = token-tree count consumed on success
            Some(consumed_tokens)
        }
        FragmentSpec::Vis | FragmentSpec::Meta => {
            // Both can be zero-width (no visibility modifier / no meta attrs).
            // Try one token (`pub`, or a `#[...]` group) else consume zero.
            if matches_fragment_spec(input.get(pos)?, spec) { Some(1) } else { Some(0) }
        }
    }
}
```
   This requires a **new public entry point in `glyim-frontend`**:
   `glyim-frontend/src/parser/mod.rs` currently only exposes
   `parse_to_syntax(source, file_id) -> ParseResult` (whole-file). Add:
   ```rust
   /// Attempt to parse exactly one fragment of the given kind starting at
   /// the beginning of `source`. Returns the number of *macro token trees*
   /// (not raw lexer tokens — the caller must map back) consumed by a
   /// successful parse, or None if `source` doesn't start with a valid
   /// fragment of this kind.
   pub fn try_parse_fragment(kind: FragmentKind, source: &str) -> Option<usize> {
       let mut parser = Parser::new(source); // reuse whatever internal Parser type parser/mod.rs already constructs for parse_to_syntax
       let ok = match kind {
           FragmentKind::Expr => parser.parse_expr_bp(0).is_ok(), // reuse parser/expr.rs's real entry point, whatever it's actually called — grep `fn parse_expr` in parser/expr.rs
           FragmentKind::Ty => parser.parse_ty().is_ok(),
           FragmentKind::Pat => parser.parse_pat().is_ok(),
           FragmentKind::Path => parser.parse_path().is_ok(),
           FragmentKind::Stmt => parser.parse_stmt().is_ok(),
           FragmentKind::Item => parser.parse_item().is_ok(),
       };
       ok.then(|| parser.tokens_consumed())
   }
   ```
   The internal parser functions (`parse_expr`-equivalent etc.) almost
   certainly already exist as non-`pub` functions in
   `parser/expr.rs`/`ty.rs`/`pat.rs`/`stmt.rs`/`item.rs` (they're used by
   `parse_to_syntax`'s dispatch) — this item is "expose + wrap them for
   fragment-boundary use", not "write a second parser".
3. **`match_pieces` calls `consume_fragment` instead of the current
   1-tree-at-a-time loop** for `PatternPiece::Fragment(spec, name)` cases —
   read `match_pieces`'s existing `PatternPiece::Fragment` arm (below line
   227 in the earlier excerpt) and replace its token-count assumption with
   `consume_fragment`'s returned count, storing that many trees under
   `bindings[name]` instead of exactly one.

**Sequencing note:** Stage A is safe to ship alone and immediately improves
error messages (rejects obviously-wrong macro invocations that today
silently "match" and then fail confusingly deep in expansion). Stage B is
the real fix and is a multi-file change — don't attempt Stage B before
Stage A is merged and tested, since Stage B's `retokenize_to_source` +
frontend entry point is new surface area worth landing separately.

**Verify:** a macro rule `($e:expr) => { ... }` invoked with `foo!(1 + 2 *
3)` — before the fix, `matches_fragment_spec` only ever looked at the
first token tree (`1`) and returned `true` unconditionally, and the
existing repetition-driving loop in `match_pieces` would misalign trying to
match subsequent pattern pieces against ` + `, ` 2`, etc. as if they were
separate top-level pieces. After Stage B: `$e` correctly captures all 5
token trees as one `expr` fragment.

---

### 4.2 `file!()`/`line!()`/`column!()` approximated from byte offset — `glyim-meta/src/expander/mod.rs`

**Current (confirmed, lines ~386-399):** `line = offset/80, col =
offset%80` — a fixed 80-column assumption, wrong for any real file.

**Fix:** `glyim-span` (already a dependency of most crates here) is exactly
the crate that should own source-position lookup — check
`glyim-span/src/lib.rs` for a `SourceMap`/`FileId`-indexed line/column
lookup (likely something like `SourceMap::lookup_line_col(file_id, byte_offset)
-> (u32, u32)`, since `glyim-diag`'s diagnostics already need this exact
capability to render `file:line:col` in error messages — grep
`glyim-diag/src/lib.rs` for how it converts a `Span` to a printable
position; reuse that exact function, don't write a second line-counter).
Replace:
```rust
// line!() expands to a line number (approximated from byte offset)
let line = offset / 80;
```
with:
```rust
let (line, _col) = source_map.lookup_line_col(file_id, offset); // whatever the real signature is
```
and the `column!()` arm similarly, using the same lookup's column output
instead of `offset % 80`.

**Verify:** a multi-line macro invocation using `line!()` on line 42 of a
test fixture must expand to the literal `42`, not
`byte_offset_of_that_point / 80`.

---

### 4.3 `include!` resolves relative to CWD, not the source file — `glyim-meta/src/expander/mod.rs`

**Current:** reads files relative to the process's current working
directory.

**Fix:** the expander already has the invoking macro call's `Span` (needed
for diagnostics on the `include!` call itself) — get that span's `FileId`,
resolve it to a filesystem path via `glyim-vfs` (check
`glyim-vfs/src/lib.rs` for a `FileId -> PathBuf`/`Vfs::file_path` lookup —
this crate exists exactly to abstract file identity from disk paths), take
`.parent()` of that path, and join the `include!` argument onto that
parent directory instead of using a bare relative path:
```rust
let calling_file_path = vfs.file_path(call_span.file_id)?; // exact method name TBD, grep glyim-vfs
let base_dir = calling_file_path.parent().unwrap_or(Path::new("."));
let include_path = base_dir.join(&include_arg);
```

**Verify:** two-file fixture, `src/main.g` (in `src/`) does
`include!("data.g")` where `data.g` also lives in `src/`; running the
compiler from the *project root* (not `src/`) must still resolve it —
today it only works if invoked from inside `src/`.

---

### 4.4 `stringify!`/`concat!` lose token structure — `glyim-meta/src/expander/mod.rs`

**Current:** `stringify!` strips outer parens but doesn't preserve original
spacing/comments.

**Scope decision:** exact source-text preservation (including original
whitespace and comments) requires each `TokenTree::Token` to carry a
source span, not just text — check whether `TokenTree::Token(SyntaxKind,
SmolStr)` (confirmed shape, `token_tree.rs` line ~5) has access to spans at
all. It doesn't (only `SyntaxKind` + text). **Full fidelity requires adding
a `Span` field to `TokenTree::Token`**, which is a bigger, more invasive
change touching every `TokenTree` construction site in this crate.

For a production-grade but scoped fix that doesn't require that
invasive change: implement the same normalization real `macro_rules!`
`stringify!` uses — deterministic re-pretty-printing (single space between
tokens, no space before `,`/`;`, space after, standard bracket spacing) —
rather than attempting byte-exact original-source reproduction:
```rust
fn stringify_trees(trees: &[TokenTree]) -> String {
    let mut out = String::new();
    for (i, tree) in trees.iter().enumerate() {
        if i > 0 && needs_space_before(&trees[i - 1], tree) {
            out.push(' ');
        }
        match tree {
            TokenTree::Token(_, text) => out.push_str(text),
            TokenTree::Group(open, inner, close) => {
                out.push_str(delim_open_str(*open));
                out.push_str(&stringify_trees(inner));
                out.push_str(delim_close_str(*close));
            }
            TokenTree::DollarCrate => out.push_str("$crate"),
        }
    }
    out
}
```
with `needs_space_before` implementing the standard punctuation-spacing
rules (no space before `,`/`;`/`)`/`]`/`}`, no space after `(`/`[`/`{`, space
everywhere else) — this matches real `stringify!` behavior closely enough
for production use and is a self-contained, low-risk change. Document the
"not byte-exact to source" limitation in a doc comment; don't attempt the
`Span`-on-`TokenTree` change as part of this item — file it separately if
byte-exact fidelity is ever actually required (it currently isn't listed
as a concrete need anywhere else in the report).

**Verify:** `stringify!(1+2)` → `"1 + 2"`; `stringify!(foo(a, b))` →
`"foo(a, b)"`.
