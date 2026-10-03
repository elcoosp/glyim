# Glyim Compiler — Complete Bug & Performance Audit

> **Repo:** https://github.com/elcoosp/glyim @ commit `eff1f1fa` ("feat(stdlib): impl FromStr for f64")
> **Scope:** ~88,000 lines of non-test Rust across 30+ crates — full pipeline: lexer → parser → def-map → HIR → typeck → THIR→MIR → borrowck → MIR opts → interpreter + LLVM & bytecode backends, plus `glyip` (build tool), `glyim-lsp`, `glyim-runtime`, `tools/glyim-pilot`.
> **Method:** every production file read line-by-line by 7 independent review passes; every finding below was verified against the actual source (exact file + line numbers at commit `eff1f1fa`). No issues are speculative.
> **Totals: 193 findings — 172 bugs, 21 performance issues.** 41 are Critical (silent wrong output, hang, memory corruption, or data loss).

---

## 0. How an AI agent should use this report

Work top-to-bottom. For each issue:

1. `cd` to the repo root (all paths are relative to it).
2. **Find** the code shown under **Where** (exact function + line range given).
3. **Apply** the fix shown in the diff block (lines with `-` are what you delete, `+` are what you write).
4. **Verify** with the command under **Check**. Do not continue until it passes.
5. After every 10 fixes, run the full suite: `cargo test --workspace` (build with `--no-default-features` if you lack LLVM 22).

Rules of engagement:

- Do NOT "improve" anything not shown in a diff. The diffs are minimal on purpose.
- If a fix doesn't compile, re-read the surrounding function — never delete error paths to make it compile.
- Every fix is designed to preserve the existing public API. If a signature change is shown, update all call sites listed in that issue.
- Severity meanings: **C** = silent miscompile / hang / memory corruption / data loss. **H** = valid programs rejected, panics, dead features, wrong results visible to users. **M** = edge-case wrongness, latent bugs that fire when adjacent code changes. **L** = minor. **P** = performance.

### The 12 fixes that matter most (do these first)

| # | Issue | One-line description |
|---|-------|----------------------|
| 1 | [RT-11](#rt-11) | Bytecode backend emits **inverted branches for every if/while/match guard** |
| 2 | [MIR-1](#mir-1) | Const-prop rewrites straight-line code with **stale constants** (wrong values) |
| 3 | [MIR-6](#mir-6) | DCE deletes assignments still needed by `Drop` → **drop of uninitialized memory** |
| 4 | [MIR-10](#mir-10) | Drop elaboration **corrupts the CFG** for array drops / drop flags |
| 5 | [LL-1](#ll-1)–[LL-4](#ll-4) | LLVM backend: **unsigned comparisons and widening casts are wrong** |
| 6 | [SOLVE-7](#solve-7) | `&&`/`||` precedence bug makes **`&i32 → &str` "coercible"** |
| 7 | [HIR-29](#hir-29) | `match` evaluates the scrutinee **twice** for non-enum scrutinees |
| 8 | [RT-21](#rt-21) | Async wake-up ID mismatch → **all async socket I/O hangs forever** |
| 9 | [INF-23](#inf-23) | Slice drop glue loop **exits immediately, drops nothing** |
| 10 | [FE-1](#fe-1) | `x %= 2` is unparseable (`PercentEq` doesn't exist) |
| 11 | [RT-3](#rt-3)/[RT-4](#rt-4) | Bytecode VM/codegen wire-format mismatches desync the instruction stream |
| 12 | [HIR-14](#hir-14) | `"\n"` unescape order corrupts strings containing `\\n` |

---

## 1. `glyim-frontend` — lexer & parser (20 findings)

The lexer/parser is the first thing every user touches; these bugs reject valid Rust-like programs or corrupt the CST.

<a name="fe-1"></a>
### [FE-1] [H] `%=` is lexed as two tokens and `PercentEq` doesn't exist at all
**Where:** `crates/glyim-frontend/src/lexer.rs:321-328`; `crates/glyim-syntax/src/lib.rs` (SyntaxKind enum)
Every other compound operator has an `Eq` branch (`lex_plus`, `lex_minus`, `lex_star`, `lex_slash`, `lex_and`, `lex_or`, `lex_lt`, `lex_gt`) but `%` never checks for `=`. The parser's assignment list (`parser/expr.rs:27-39`) also has no `PercentEq`. Input `x %= 2;` lexes as `Percent, Eq`, then the multiplicative loop consumes `%` as a binary operator and `parse_primary_expr` errors on `=`. Valid program → cascading errors.

**Fix (2 steps):**

Step 1 — `crates/glyim-syntax/src/lib.rs`, add a variant next to `SlashEq`:
```diff
     SlashEq,
+    PercentEq,
```
(If `kind_to_raw` maps by `self as u16`, nothing else needed there.)

Step 2 — `crates/glyim-frontend/src/lexer.rs:321`, replace the `'%'` arm:
```diff
                 '%' => {
                     self.advance();
-                    tokens.push(Token::new(
-                        SyntaxKind::Percent,
-                        self.span(start, self.pos),
-                        "%",
-                    ));
+                    if self.peek() == Some('=') {
+                        self.advance();
+                        tokens.push(Token::new(
+                            SyntaxKind::PercentEq,
+                            self.span(start, self.pos),
+                            "%=",
+                        ));
+                    } else {
+                        tokens.push(Token::new(
+                            SyntaxKind::Percent,
+                            self.span(start, self.pos),
+                            "%",
+                        ));
+                    }
                 }
```
Step 3 — `crates/glyim-frontend/src/parser/expr.rs:27-39`, add `| SyntaxKind::PercentEq` to the `matches!` list of assignment operators.

**Check:** `cargo test -p glyim-frontend` then compile `fn main() { let mut x = 5; x %= 2; }` — zero diagnostics.

<a name="fe-2"></a>
### [FE-2] [H] Tuple `(1,)` double-errors and swallows the `)`
**Where:** `crates/glyim-frontend/src/parser/expr.rs:636-650`
The tuple loop unconditionally calls `parse_expr()` after each comma, unlike call args (`expr.rs:309-317`) and array exprs (`expr.rs:719-725`) which check for the closing delimiter. For `(1,)` the parser emits "expected expression, found RParen", **bumps the `)`**, then `expect(RParen)` fires a second error.

**Fix:**
```diff
                         self.start_node_at(cp, SyntaxKind::TupleExpr);
                         while self.current_kind() == SyntaxKind::Comma {
                             self.bump();
+                            if self.current_kind() == SyntaxKind::RParen {
+                                break;
+                            }
                             self.parse_expr();
                         }
```

**Check:** parse `let t = (1,);` — zero diagnostics, `TupleExpr` contains both elements.

<a name="fe-3"></a>
### [FE-3] [H] Chained casts `x as u8 as u32` are rejected (`if` instead of `while`)
**Where:** `crates/glyim-frontend/src/parser/expr.rs:196-205`
`as` is left-associative in Rust and freely chains. Glyim parses exactly one cast; the second `KwAs` reaches `parse_stmt`'s fallback → "unexpected token in statement".

**Fix:**
```diff
     pub(crate) fn parse_cast_expr(&mut self) {
         let cp = self.checkpoint();
         self.parse_unary_expr();
-        if self.current_kind() == SyntaxKind::KwAs {
+        while self.current_kind() == SyntaxKind::KwAs {
             self.start_node_at(cp, SyntaxKind::CastExpr);
             self.bump();
             self.parse_type();
             self.finish_node();
         }
     }
```
The `start_node_at(cp, …)` re-wrap is the same left-associative pattern already used in `parse_or_expr`.

**Check:** parse `let y = 1 as i64 as f64;` — zero diagnostics.

<a name="fe-4"></a>
### [FE-4] [H] `Vec<u8>::new()` (generic path without turbofish) doesn't parse
**Where:** `crates/glyim-frontend/src/parser/expr.rs:756-790`
Generic args (`Lt`) are handled only after `::` (turbofish) and in `PathType`. After the first path segment in an *expression*, no layer handles `Lt`, so `<u8>::new()` dangles.

**Fix:** in `parse_path_inner`, after the first-segment `bump()` and before the `while … ColonColon` loop:
```diff
         while self.current_kind() == SyntaxKind::ColonColon {
```
insert:
```diff
+        if self.current_kind() == SyntaxKind::Lt {
+            self.parse_type_arg_list();
+        }
```
(Use the same helper the turbofish arm calls at `expr.rs:777-779`; the existing `pending_gt_count` `>>` splitting already balances nested `Vec::<Vec<u8>>`.)

**Check:** parse `let v = Vec<u8>::new();` — zero diagnostics.

<a name="fe-5"></a>
### [FE-5] [H] Negative literals/range patterns silently lose their sign
**Where:** `crates/glyim-frontend/src/parser/pat.rs:164-217`
Neither `parse_pat_single` nor `parse_pat_inner` has a `Minus` arm. For `match n { -3..=3 => …, _ => … }` the `-` hits the error arm and is bumped, then `3..=3` parses as a `PatRange` — **the accepted tree has the wrong semantics** (matches 3..=3, not -3..=3).

**Fix:** in `parse_pat_inner`, before the literal arm, add:
```diff
+            SyntaxKind::Minus => {
+                let cp = self.checkpoint();
+                self.start_node_at(cp, SyntaxKind::PatLit);
+                self.bump(); // '-'
+                // fall through to literal parsing by recursing:
+                self.parse_pat_inner();
+                self.finish_node(); // PatLit (sign included in node text)
+            }
```
And in the range arm (`pat.rs:170-204`) accept `Minus? IntLit` for both endpoints so `-3..=3` keeps the sign inside the `PatRange` node.

**Check:** parse `match n { -3..=3 => a, _ => b }` — the range token text must contain `-3`.

<a name="fe-6"></a>
### [FE-6] [C] Stack-overflow DoS: depth guard covers only `(...)` — unary chains, arrays, blocks, patterns, token-trees recurse unbounded
**Where:** `crates/glyim-frontend/src/parser/expr.rs:613-627` (the only guard site); `parser/mod.rs:45`
`recursion_depth` is incremented in exactly one place. `- - - - … x` (200k minuses), `[[[[…]]]]`, `{{{{…}}}}`, nested patterns, and `macro_rules! m { ( ((((…)))) ) => {} }` all overflow the stack → compiler crash instead of the intended diagnostic. The `no-ice` corpus files `deep_arrays.g`/`deep_parens.g`/`deep_if.g` are all **0 bytes**, and `depth_limit.rs` only tests `(`.

**Fix:** move the check into the two real recursive entry points. In `parser/expr.rs`:

```diff
     pub(crate) fn parse_expr(&mut self) {
+        if self.recursion_depth > super::MAX_EXPR_DEPTH {
+            self.error("expression nested too deeply");
+            return;
+        }
+        self.recursion_depth += 1;
         // ... existing body ...
+        self.recursion_depth -= 1;
     }
```
Do the same in `parse_block` (expr.rs:6-12), and in `parser/pat.rs::parse_pat_inner` + `parser/item.rs::parse_token_tree` add the same guard using the same `MAX_EXPR_DEPTH` constant. Delete the now-redundant guard at expr.rs:613-627 (or leave it; it becomes harmless).

**Check:** generate a 500k-deep `-` chain and parse it — must produce the depth error, not SIGSEGV.

<a name="fe-7"></a>
### [FE-7] [H] `:literal` fragments reject `true`/`false` — `BoolLit` is never produced
**Where:** `crates/glyim-frontend/src/parser/mod.rs:406-418, 589-600`; `lexer.rs:918-919`; `crates/glyim-syntax/src/lib.rs:417-427`
`lookup_keyword` maps `true|false` → `KwTrue|KwFalse`, but `is_literal()` only accepts `BoolLit` — a kind nothing produces. So `m!(true)` for `($l:literal)` fails Stage-B matching.

**Fix (choose one, (a) is smaller):**
(a) `crates/glyim-syntax/src/lib.rs:417-427`, extend `is_literal`:
```diff
     pub fn is_literal(&self) -> bool {
         matches!(
             self,
-            SyntaxKind::BoolLit,
+            SyntaxKind::BoolLit | SyntaxKind::KwTrue | SyntaxKind::KwFalse,
```
plus update `is_meta_literal` (mod.rs:590-600) to accept `KwTrue|KwFalse`, and fix the test pinning the wrong behavior at `crates/glyim-syntax/src/tests/…/kind_tests.rs:97`.

**Check:** `macro_rules! m { ($l:literal) => { 1 } } fn main() { let _ = m!(true); }` expands.

<a name="fe-8"></a>
### [FE-8] [H] `&&expr` is unparseable in expressions; `&&mut T` type silently loses `mut`
**Where:** `crates/glyim-frontend/src/parser/expr.rs:207-227`; `parser/ty.rs:8-21`
The lexer coalesces `&&` into one `AndAnd` token; the unary arm matches only `And`. So `let r = &&5;` → "expected expression, found AndAnd". The parser *does* split `AndAnd` in `parse_type` and `parse_pat_single`, proving the expr omission is an oversight. Worse: the ty.rs `AndAnd` branch emits two synthetic `&` then calls `parse_type()` directly with no `mut` handling, so `&&mut T` errors on `KwMut` and yields `&&T` — a silent mutability change.

**Fix (expr):** in `parse_unary_expr`, mirror ty.rs:
```diff
         if matches!(
             self.current_kind(),
-            SyntaxKind::Bang | SyntaxKind::Minus | SyntaxKind::Star | SyntaxKind::And
+            SyntaxKind::Bang | SyntaxKind::Minus | SyntaxKind::Star | SyntaxKind::And
+                | SyntaxKind::AndAnd
         ) {
             let cp = self.checkpoint();
             self.start_node_at(cp, SyntaxKind::UnaryExpr);
+            if self.current_kind() == SyntaxKind::AndAnd {
+                self.bump_synthetic_and(); // emit two '&' tokens exactly like ty.rs:8-21
+            } else {
             self.bump();
+            }
             self.parse_unary_expr();
             self.finish_node();
```
**Fix (ty):** in `parser/ty.rs` `AndAnd` branch, copy the `mut`/lifetime loop from the single-`&` branch (ty.rs:22-40) before the inner `parse_type()`.

**Check:** `let r = &&5;` and a fn signature `f(x: &&mut i32)` both parse.

<a name="fe-9"></a>
### [FE-9] [H] `@` bindings (`x @ 1..=5`) misparse with cascading errors
**Where:** `crates/glyim-frontend/src/parser/pat.rs:96-162`
No `At` handling anywhere in the pattern grammar. `x @ 1..=5` → `x` becomes `PatIdent`, `expect(FatArrow)` fails on `@`, then `1..=5` is consumed as the arm *body* and the real `=> a` becomes a broken next arm.

**Fix:** in the Ident arm of `parse_pat_inner`, after building `PatIdent`:
```diff
+                if self.current_kind() == SyntaxKind::At {
+                    self.start_node_at(pat_cp, SyntaxKind::PatIdent);
+                    self.bump(); // '@'
+                    self.parse_pat_inner();
+                    self.finish_node(); // PatIdent (binding @ subpattern)
+                }
```
**Check:** parse `match n { x @ 1..=5 => a, _ => b }` — one arm, binding with sub-pattern.

<a name="fe-10"></a>
### [FE-10] [H] Labeled `break 'a value;` / `continue 'a;` produce parse errors
**Where:** `crates/glyim-frontend/src/parser/expr.rs:686-707`
`parse_label` exists (expr.rs:457-467) and is used for loop prefixes, but the `KwBreak`/`KwContinue` arms never consult it. `continue 'outer;` leaves the `Lifetime` to hit "expected ';'".

**Fix:** in both arms, right after `self.bump(); // break/continue`:
```diff
                 self.start_node(SyntaxKind::BreakExpr);
                 self.bump(); // break
+                self.parse_label(); // no-op when no label follows
```
**Check:** parse `loop { continue 'outer; }` inside a labeled loop and `break 'a 1;` — zero diagnostics.

<a name="fe-11"></a>
### [FE-11] [L] Leading `|` in a match arm emits a spurious "expected pattern"
**Where:** `crates/glyim-frontend/src/parser/pat.rs:5-16`
`match x { | 1 => a, _ => b }` is valid Rust. `parse_pat` starts with `parse_pat_single`, which has no `Or` arm.

**Fix:**
```diff
     pub(crate) fn parse_pat(&mut self) {
         let cp = self.checkpoint();
+        if self.current_kind() == SyntaxKind::Or {
+            self.bump();
+        }
         self.parse_pat_single();
```
**Check:** parse `match x { | 1 => a, _ => b }` — zero diagnostics.

<a name="fe-12"></a>
### [FE-12] [M] `pub(in path)` visibility is rejected by the parser (the `:vis` macro matcher accepts it!)
**Where:** `crates/glyim-frontend/src/parser/item.rs:203-231`
No `KwIn` arm → two errors + cascades for `pub(in crate::foo) fn f() {}`. Inconsistently, the `:vis` fragment matcher (parser/mod.rs:446-448) *does* handle `KwIn`.

**Fix:**
```diff
             match self.current_kind() {
                 SyntaxKind::KwCrate => { … }
                 SyntaxKind::KwSuper => { … }
                 SyntaxKind::KwSelf => { … }
+                SyntaxKind::KwIn => {
+                    self.bump(); // 'in'
+                    // parse path, same node kind as the Ident arm (VisPath)
+                }
                 SyntaxKind::Ident => { … }
```
**Check:** parse `pub(in crate::foo) fn f() {}` — zero diagnostics.

<a name="fe-13"></a>
### [FE-13] [M] `extern crate name;` leaves the semicolon unconsumed
**Where:** `crates/glyim-frontend/src/parser/item.rs:133-139`
The `KwCrate` branch of `extern` items consumes `crate`/name/`as alias` but never `expect(Semicolon)`.

**Fix:**
```diff
                 if self.current_kind() == SyntaxKind::KwCrate {
                     self.bump(); // crate
                     self.bump_expected(SyntaxKind::Ident); // crate name
                     if self.current_kind() == SyntaxKind::KwAs {
                         self.bump(); // as
                         self.bump_expected(SyntaxKind::Ident); // alias
                     }
+                    self.expect(SyntaxKind::Semicolon);
                 }
```
**Check:** parse `extern crate foo;` — zero diagnostics.

<a name="fe-14"></a>
### [FE-14] [M] `const _: T = …;` (anonymous const) emits a spurious error
**Where:** `crates/glyim-frontend/src/parser/item.rs:67-71`
`bump_expected(Ident)` rejects `_` (lexed as `Underscore`).

**Fix:**
```diff
                 self.start_node(SyntaxKind::ConstDef);
                 self.bump(); // const
-                self.bump_expected(SyntaxKind::Ident);
+                if matches!(self.current_kind(), SyntaxKind::Ident | SyntaxKind::Underscore) {
+                    self.bump();
+                } else {
+                    self.error("expected name or `_` after `const`");
+                }
```
Apply the same to `static _` at item.rs:89. **Check:** parse `const _: () = ();` — zero diagnostics.

<a name="fe-15"></a>
### [FE-15] [M] `fn f(&x: &i32)` misparsed; tokens emitted after the `Param` node closed
**Where:** `crates/glyim-frontend/src/parser/item.rs:476-484`
After `finish_node(); // Param`, the code still bumps an `Ident`, expects `:`, and parses a type — those land *outside* the closed node. For valid pattern-params the `Param` closes as `&x` with two spurious errors.

**Fix:** delete the trailing three lines:
```diff
             self.start_node(SyntaxKind::RefType);
             self.parse_type();
             self.finish_node(); // RefType
             self.finish_node(); // Param
-            self.bump_expected(SyntaxKind::Ident);
-            self.expect(SyntaxKind::Colon);
-            self.parse_type();
```
(If pattern-typed params are wanted, parse `parse_pat_single()` then `:` then type *before* `finish_node`.)

<a name="fe-16"></a>
### [FE-16] [M] Radix literals with no digits (`0x`, `0b`, `0o`) lex clean with no diagnostic
**Where:** `crates/glyim-frontend/src/lexer.rs:509-547`
Unlike the exponent path (which diagnoses `1e` and rolls back, lexer.rs:590-602), hex/bin/oct emit no error when zero digits follow the prefix; `0x` becomes a clean `IntLit`. `0x_` also passes (underscore-only "digits").

**Fix:** in each radix branch, count real digits:
```diff
                 self.advance();
                 self.advance();
+                let mut real_digits = 0usize;
                 while let Some(ch) = self.peek() {
                     if ch.is_ascii_hexdigit() || ch == '_' {
+                        if ch != '_' { real_digits += 1; }
                         self.advance();
                     } else {
                         break;
                     }
                 }
+                if real_digits == 0 {
+                    self.lex_error(self.span(start, self.pos), "no valid digits in number");
+                }
                 self.lex_number_suffix();
```
**Check:** `let x = 0x;` produces a lex error spanned over `0x`.

<a name="fe-17"></a>
### [FE-17] [M] `1e_` is accepted as a float (underscore satisfies the exponent check)
**Where:** `crates/glyim-frontend/src/lexer.rs:581-589`
`has_exponent_digits` is set for `_` too. Rust requires ≥1 real digit after `e`.

**Fix:**
```diff
             let mut has_exponent_digits = false;
             while let Some(ch) = self.peek() {
-                if ch.is_ascii_digit() || ch == '_' {
+                if ch.is_ascii_digit() {
                     self.advance();
                     has_exponent_digits = true;
+                } else if ch == '_' && has_exponent_digits {
+                    self.advance();
                 } else {
                     break;
                 }
             }
```
**Check:** `let x = 1e_;` errors; `1e1_0` parses.

<a name="fe-18"></a>
### [FE-18] [M] Unterminated block comment produces no diagnostic
**Where:** `crates/glyim-frontend/src/lexer.rs:468-490`
The nesting loop breaks on EOF without pushing an error (unlike strings/chars).

**Fix:** capture `let comment_start = start;` before the loop; after it:
```diff
                 let mut depth = 1u32;
                 while depth > 0 {
                     match self.peek() {
                         …
                         None => break,
                     }
                 }
+                if depth > 0 {
+                    self.lex_error(self.span(comment_start, self.pos), "unterminated block comment");
+                }
```
**Check:** `fn main() {} /* oops` yields the error.

<a name="fe-19"></a>
### [FE-19] [P] `bump()`/`flush_trivia()` clone every token's `SmolStr` in the hottest parser loop
**Where:** `crates/glyim-frontend/src/parser/mod.rs:132-146, 73-84`
`self.tokens` is `&'a [Token]` (independent lifetime), so borrowing compiles without the clone. Every token in every file pays a memcpy/refcount.

**Fix:**
```diff
         if let Some(token) = self.tokens.get(self.pos) {
             let kind = GlyimLang::kind_to_raw(token.kind);
-            let text = token.text.clone();
-            self.builder.token(kind, text.as_str());
+            self.builder.token(kind, token.text.as_str());
             self.pos += 1;
         }
```
Same change in `flush_trivia`. **Check:** `cargo test -p glyim-frontend` still green; parser gets a free ~5-10% speedup.

<a name="fe-20"></a>
### [FE-20] [M] `is_keyword()`/`is_node()` use numeric ranges that exclude real variants
**Where:** `crates/glyim-syntax/src/lib.rs:430-439`
`KwAsync`/`KwAwait`/`Lifetime` are declared *after* `KwMacroRules`, so `is_keyword()` returns false for real keywords async/await. `Visibility`…`MetaVarCrate` are declared after `Error`, so `is_node()` misses real node kinds. Tests hide it because their hardcoded lists omit the same variants.

**Fix:** replace the range checks with explicit lists:
```diff
     pub fn is_keyword(&self) -> bool {
-        let raw = *self as u16;
-        raw >= SyntaxKind::KwFn as u16 && raw <= SyntaxKind::KwMacroRules as u16
+        matches!(self, SyntaxKind::KwFn | … | SyntaxKind::KwAsync | SyntaxKind::KwAwait | SyntaxKind::Lifetime)
     }
```
and the same for `is_node` (list every node kind explicitly). Extend `kind_tests.rs` to iterate an exhaustive `const ALL: [SyntaxKind; N]`.

---

## 2. `glyim-solve` + typeck unification (23 findings)

The trait solver and unification core. Several bugs here can hang the compiler or silently accept ill-typed programs.

<a name="solve-1"></a>
### [SOLVE-1] [C] Int-var arms create binding cycles → stack overflow / spurious "infinite type"
**Where:** `crates/glyim-solve/src/infer.rs:228-284` (Int arm; Float arm identical at 262-284)
`unify_tys` binds int vars to the *raw* peer type without resolving it first. Unifying `(I1,I2)` with `(I2,I1)` (e.g. `if c { (a,b) } else { (b,a) }` where both are fresh int vars) makes `int_vars[I1] := Infer(Int(I2))` and `int_vars[I2] := Infer(Int(I1))` — a cycle. Every later `unify` then recurses forever in `resolve_ty_shallow_preserve_int` (no cycle guard, see SOLVE-3) → stack overflow; or zonk reports a bogus "infinite type" for well-typed code.

**Fix (do together with SOLVE-2/3 — one root fix):** in both the Int and Float arms:
```diff
-                let other_ty = if a_is_int { b } else { a };
+                let other_ty = self.resolve_ty_shallow_preserve_int(
+                    ctx,
+                    if a_is_int { b } else { a },
+                );
                 let int_var_ty = if a_is_int { a } else { b };
-                match &other {
+                if other_ty == int_var_ty {
+                    return Ok(Vec::new()); // already same var / already bound to it
+                }
+                match &other {
```
**Check:** `cargo test -p glyim-solve` plus a new test unifying `(a,b)` with `(b,a)` for two fresh int vars — must succeed.

<a name="solve-2"></a>
### [SOLVE-2] [C] `unify_tys` rebinds already-bound vars without following chains — silent overwrites, spurious occurs errors, `unify(a,b) ≠ unify(b,a)`
**Where:** `crates/glyim-solve/src/infer.rs:210-219, 285-311`
Only the public `unify()` pre-resolves its args; every internal recursive call (Tuple arm :582, Adt :740, FnPtr :681, Ref :509) passes raw element tys. Failure A: `?A := ?Z := i32`, then a composite unification reaching `unify_tys(?A, u32)` **overwrites** `ty_vars[?A] := u32`, silently discarding the `i32` constraint instead of erroring. Failure B: unify `Adt(S,[?A])` vs `Adt(S,[?B])` then the same pair reversed — `occurs(?B, ?A)` misfires because `?A` isn't resolved, reporting "cannot construct infinite type" for a legal unification.

**Fix:** at the top of `unify_tys` (infer.rs:210):
```diff
     fn unify_tys(&mut self, ctx: &mut TyCtxMut, a: Ty, b: Ty, span: Span)
         -> Result<Vec<Constraint>, Vec<GlyimDiagnostic>> {
         if a == b { return Ok(Vec::new()); }
+        let a = self.resolve_ty_shallow_preserve_int(ctx, a);
+        let b = self.resolve_ty_shallow_preserve_int(ctx, b);
+        if a == b { return Ok(Vec::new()); }
         let a_kind = ctx.ty_kind(a).clone();
```
and in the general-var arm (:285-311) skip binding when the resolved other side is the var itself:
```diff
+                if other_ty == a || other_ty == b { return Ok(Vec::new()); }
                 self.ty_vars[var].value = Some(other_ty);
```
**Check:** the two scenarios above as new unit tests — both must succeed (first) / unify cleanly (second).

<a name="solve-3"></a>
### [SOLVE-3] [H] `resolve_ty_shallow_preserve_int` has no depth limit or cycle detection
**Where:** `crates/glyim-solve/src/infer.rs:1091-1116`
The sibling `resolve_ty_shallow_depth` (:1118-1178) has `MAX_RESOLVE_DEPTH` + a visited set; this variant — the first thing every public `unify()` executes — has neither. Any var cycle turns into unbounded recursion → process abort. Same unguarded recursion in `has_unresolved_non_ty_infer` (:1199-1238) and `collect_unresolved_vars` (:1240-1277).

**Fix:** add the same guards as `resolve_ty_shallow_depth`:
```diff
+    const MAX_PRESERVE_DEPTH: u32 = 256;
     pub fn resolve_ty_shallow_preserve_int(&self, ctx: &dyn TypeLookup, ty: Ty) -> Ty {
+        self.resolve_preserve_int_inner(ctx, ty, &mut std::collections::HashSet::new(), 0)
+    }
+    fn resolve_preserve_int_inner(&self, ctx: &dyn TypeLookup, ty: Ty,
+        seen: &mut std::collections::HashSet<u32>, depth: u32) -> Ty {
+        if depth > MAX_PRESERVE_DEPTH || !seen.insert(ty.index()) { return Ty::ERROR; }
         match ctx.ty_kind(ty) {
             TyKind::Infer(InferVar::Int(var)) => {
                 if let Some(value) = self.int_vars.get(*var).and_then(|v| v.value) {
-                    self.resolve_ty_shallow_preserve_int(ctx, value)
+                    self.resolve_preserve_int_inner(ctx, value, seen, depth + 1)
```
(mirror for the Ty-var arm; apply the same visited-set treatment to the two collectors).

<a name="solve-4"></a>
### [SOLVE-4] [H] Ref-arm speculative deref/Vec coercions mutate the table with no snapshot/rollback
**Where:** `crates/glyim-solve/src/infer.rs:446-508`
Each speculative `unify_tys` probe binds inference vars as it goes; on partial failure the bindings are never undone, poisoning the table (e.g. a failed deref probe leaves `?x := u8` committed). Same pattern at `check_expr.rs:3362-3364` (three chained `.is_ok()` probes) and :3010-3024 (successful probe not rolled back when trait resolution then fails).

**Fix:** wrap each speculative attempt:
```diff
+                    let snap = self.snapshot();
                     let deref_a = ctx.deref_ty(ty_a);
                     if let Some(da) = deref_a {
                         if self.unify_tys(ctx, da, ty_b, span).is_ok() {
                             return Ok(constraints);
                         }
                     }
+                    self.rollback_to(snap);
```
(Repeat before `deref_b` and each vec-to-slice attempt; snapshot is cheap — see SOLVE-21 for making it cheaper.)

**Check:** `cargo test -p glyim-solve` — existing coercion tests must stay green.

<a name="solve-5"></a>
### [SOLVE-5] [H] Occurs check doesn't walk `Projection`/`Dynamic` → self-referential types pass
**Where:** `crates/glyim-solve/src/infer.rs:135-169`
`TyKind::Projection` and `TyKind::Dynamic` fall into `_ => false`. A goal `?X = <?X as Iterator>::Item` passes the occurs check and binds a self-referential type, deferring the explosion to layout/mono (`UnknownType(Ty::ERROR)`).

**Fix:**
```diff
             TyKind::Opaque(_, substs) => {
                 for arg in ctx.substitution_args(*substs) { … }
                 false
             }
+            TyKind::Projection(proj) => {
+                for arg in ctx.substitution_args(proj.trait_ref.substs) {
+                    if let GenericArg::Ty(t) = arg && self.occurs(ctx, var, *t) { return true; }
+                }
+                false
+            }
+            TyKind::Dynamic(preds, _) => {
+                preds.skip_binder().iter().any(|p|
+                    matches!(p, Predicate::Trait(t)
+                        if ctx.substitution_args(t.trait_ref.substs).any(
+                            |a| matches!(a, GenericArg::Ty(t2) if self.occurs(ctx, var, *t2)))))
+            }
```
(Adjust to the crate's actual `Projection`/`Dynamic` field names in `crates/glyim-type/src/ty.rs`.)

<a name="solve-6"></a>
### [SOLVE-6] [H] `process_obligations` silently drops `Coerce`/`WellFormed`/`TypeOutlives`/`RegionOutlives` obligations
**Where:** `crates/glyim-solve/src/fulfill.rs:148-168`
These four kinds match an empty arm — every registered obligation of these kinds is discarded unconditionally. Coercion obligations are "proven" without evaluation although `can_coerce` exists (solver.rs:580-586).

**Fix:**
```diff
                 Predicate::WellFormed(_)
                 | Predicate::TypeOutlives(_)
-                | Predicate::RegionOutlives(_)
-                | Predicate::Coerce(_, _) => {}
+                | Predicate::RegionOutlives(_) => { /* region wf: defer until regions land (SOLVE-11) */ }
+                Predicate::Coerce(from, to) => {
+                    if !crate::solver::can_coerce(self.ctx, *from, *to) {
+                        self.pending_errors.push(/* mismatched-types diagnostic for this obligation */);
+                    }
+                }
```
At minimum, `Coerce` must be evaluated; the other three can stay deferred but should get a `tracing::debug!` so the gap is visible.

<a name="solve-7"></a>
### [SOLVE-7] [C] `can_coerce`: `&&` binds tighter than `||` — pointee check skipped when ref mutabilities match
**Where:** `crates/glyim-solve/src/solver.rs:507-518` and an identical copy in `crates/glyim-solve/src/fulfill.rs:67-79`
```rust
(mut_a == mut_b) || (*mut_a == Mut && *mut_b == Not) && can_coerce(ctx, *inner_a, *inner_b)
```
parses as `(mut_a == mut_b) || (… && can_coerce(inner))`. When both are `Not` (or both `Mut`), the result is `true` **without ever comparing pointees**: `&i32 → &str`, `&Vec<u8> → &'static str`, `&mut T → &mut U` all report "coercible".

**Fix (both copies):**
```diff
-            (mut_a == mut_b)
-                || (*mut_a == glyim_core::primitives::Mutability::Mut
-                    && *mut_b == glyim_core::primitives::Mutability::Not)
-                    && can_coerce(ctx, *inner_a, *inner_b)
+            ((mut_a == mut_b)
+                || (*mut_a == glyim_core::primitives::Mutability::Mut
+                    && *mut_b == glyim_core::primitives::Mutability::Not))
+                && can_coerce(ctx, *inner_a, *inner_b)
```
Better: delete the `fulfill.rs` duplicate and `pub use crate::solver::can_coerce;`. Also apply the identical fix to the `RawPtr` arm directly below.

**Check:** new test: `can_coerce(ctx, ty_ref_i32, ty_ref_str) == false`.

<a name="solve-8"></a>
### [SOLVE-8] [C] Trait solver recursion with no cycle detection or depth limit → stack overflow on `impl<T: Foo> Foo for T {}`
**Where:** `crates/glyim-solve/src/solver.rs:354-482` (recursion site 450-451)
`prove_trait` → matched impl → `evaluate_predicate` → `can_prove` → `prove_trait` recurses with no in-progress-goal set and no depth counter. A self-referential blanket impl matches itself forever → compiler abort. (The only bound, fulfill.rs's 100_000 obligation cap, is never reached because the crash happens inside one `can_prove` call.)

**Fix:** thread a depth + visited set:
```diff
-    pub fn prove_trait(&mut self, ctx: &mut TyCtxMut, goal: &TraitRef) -> SolverResult {
+    pub fn prove_trait(&mut self, ctx: &mut TyCtxMut, goal: &TraitRef) -> SolverResult {
+        self.prove_trait_inner(ctx, goal, 0, &mut std::collections::HashSet::new())
+    }
+    fn prove_trait_inner(&mut self, ctx: &mut TyCtxMut, goal: &TraitRef,
+        depth: usize, in_progress: &mut std::collections::HashSet<(u32, Ty)>) -> SolverResult {
+        if depth > 128 { return SolverResult::Ambiguous; }
+        let key = (goal.def_id.to_raw(), goal.self_ty);
+        if !in_progress.insert(key) { return SolverResult::Ambiguous; } // cycle
         …
-            for pred in &impl_def.predicates {
-                match self.evaluate_predicate(ctx, pred) {
+            let result = self.evaluate_predicate_inner(ctx, pred, depth + 1, in_progress);
```
(Do the same threading inside `evaluate_predicate`. On all exits, `in_progress.remove(&key);`.)

**Check:** compile `trait Foo {} impl<T: Foo> Foo for T {} fn f<T: Foo>(x: T) {}` — diagnostic or clean ambiguity, **no crash**.

<a name="solve-9"></a>
### [SOLVE-9] [H] An inference-var self type matches *any* impl and commits — no deferral
**Where:** `crates/glyim-solve/src/solver.rs:273-278`
Goal `?X: Pretty` matches every impl; with one impl registered, `prove_trait` returns `Proven` without constraining `?X`, and the obligation is never rechecked (obligations process exactly once, fulfill.rs:139-169). With two impls it becomes a hard error even when later inference would disambiguate.

**Fix:** in `prove_trait` before impl selection:
```diff
+        if ctx.ty_flags(goal.self_ty).contains(TypeFlags::HAS_TY_INFER) {
+            return SolverResult::Ambiguous; // defer until the var is bound
+        }
```
and in `crates/glyim-solve/src/fulfill.rs` treat `Ambiguous` as "re-queue at the end of the queue" instead of an immediate diagnostic (keep a per-obligation retry counter; error after e.g. 3 full passes with no progress).

<a name="solve-10"></a>
### [SOLVE-10] [M] HRTB `region_map` indexed by region-ordinal but looked up by full bound-var index
**Where:** `crates/glyim-solve/src/hrtb.rs:33-47, 57-83, 110-123`
`PlaceholderRegion.index` stores the full binder index; `build_region_substitution` pushes placeholders in region-ordinal order. For a mixed binder `[Ty, Region]` the region sits at full index 1 while `region_map.len() == 1` → `get(1) = None` → the late-bound region is silently *not* substituted. Latent today (no non-test producer of `LateBound` yet) but guaranteed to break the moment HRTB fn signatures are produced.

**Fix:** build the map with full-list indices:
```diff
-            let mut region_map: Vec<Region> = Vec::new();
-            for (idx, var) in bound_vars.iter().enumerate() { … push … }
+            let mut region_map: Vec<Option<Region>> = vec![None; bound_vars.len()];
+            for (idx, var) in bound_vars.iter().enumerate() {
+                if let BoundVariableKind::Region(kind) = var {
+                    region_map[idx] = Some(Region::Placeholder(PlaceholderRegion { universe, bound: kind.clone(), index: idx as u32 }));
+                }
+            }
```
and in the `Region::LateBound` substitution arm handle `Option`:
```diff
-                if let Some(replacement) = sub.region_map.get(idx as usize) { replacement.clone() } else { self }
+                match sub.region_map.get(idx as usize) { Some(Some(r)) => r.clone(), _ => self }
```
Also fix the test at `crates/glyim-solve/src/tests/hrtb.rs:504-522` which hand-writes `LateBound(INNERMOST, 0, …)` for a var at position 1 — it bakes in the wrong convention.

<a name="solve-11"></a>
### [SOLVE-11] [H] Region constraints produced by `unify` are discarded by every caller
**Where:** `crates/glyim-typeck/src/unify.rs:29-40` (producers: infer.rs :436, :922-926, :947-966)
`InferenceTable::unify` returns `Ok(Vec<Constraint>)` — `RegionEq` constraints from every `Ref` unification — but `FnCtxt::unify` matches `Ok(_) => true` and drops them. The region half of unification is computed and thrown away; region vars can never be solved.

**Fix:**
```diff
+    pub region_constraints: Vec<Constraint>,   // new field on FnCtxt
     pub fn unify(&mut self, a: Ty, b: Ty, span: Span) -> bool {
         if a == Ty::ERROR || b == Ty::ERROR { return false; }
         match self.infer.unify(self.ctx, a, b, span) {
-            Ok(_) => true,
+            Ok(constraints) => { self.region_constraints.extend(constraints); true }
             Err(diags) => { self.diagnostics.extend(diags); false }
         }
     }
```
**Check:** `cargo test -p glyim-typeck` green; add a debug assertion test that a `&'a i32 ↔ &'b i32` unify yields one `RegionEq`.

<a name="solve-12"></a>
### [SOLVE-12] [H] `lookup_field_ty` returns the declared field type without substituting the ADT's generic arguments
**Where:** `crates/glyim-typeck/src/unify.rs:42-55`
For `struct Wrapper<T> { v: T }` matched by pattern `Wrapper { v }` against `Wrapper<u64>`, this returns rigid `Param(T)` — the pattern binds `v: T`. The expression-side equivalent (`lookup_field_ty_with_substs`, check_expr.rs:3394-3427) *does* substitute; pattern destructuring doesn't.

**Fix:** at the call site (`crates/glyim-typeck/src/check_pat.rs:366-373`), extract the substs from the expected type and use the substituting variant:
```diff
-        let field_ty = self.lookup_field_ty(adt_id, field_name, span);
+        let field_ty = match self.ctx.ty_kind(expected_ty) {
+            TyKind::Adt(_, substs) => self.lookup_field_ty_with_substs(adt_id, field_name, substs, span),
+            _ => self.lookup_field_ty(adt_id, field_name, span),
+        };
```
**Check:** new typeck test: `match w { Wrapper { v } => v + 1u64 }` type-checks as `u64`.

<a name="solve-13"></a>
### [SOLVE-13] [H] `instantiate_fn_sig` fallback returns the *first* fn item's return type in the crate
**Where:** `crates/glyim-typeck/src/unify.rs:983-1010`
When `fn_sig(def_id)` is `None` (builtins like synthetic `drop`), the loop ignores `_id` and returns the return type of the **first `fn` item in the whole crate**. `fn helper() -> i32` above `main` makes `drop(x)` type as `i32`.

**Fix:**
```diff
         for (_id, item) in self.hir.items.iter_enumerated() {
-            if let glyim_hir::ItemKind::Fn(fn_item) = &item.kind {
-                if let Some(return_ty_ref) = &fn_item.return_ty {
-                    …
-                } else {
-                    return Ty::UNIT;
-                }
-            }
+            // No fallback: an unknown signature is an internal bug, not another fn's signature.
+            let _ = item;
         }
+        // Builtins without a registered sig are unit-procducing until registered properly:
+        tracing::warn!("no FnSig registered for def_id {:?}", def_id);
+        Ty::UNIT
```
**Check:** `fn helper() -> i32 { 1 } fn main() { drop(5u8); }` — `drop` call types as `()`.

<a name="solve-14"></a>
### [SOLVE-14] [M] Explicit `5i32`/`5usize` literals become inference vars — suffix silently ignored
**Where:** `crates/glyim-typeck/src/unify.rs:1013-1035`
`Literal::Int(_, Some(IntTy::I32))` shares the `None` arm, so `let a = 5i32; let b: u32 = a;` compiles (Rust rejects). Every other suffix stays concrete; this is internally inconsistent.

**Fix:** change the parser/HIR to distinguish "no suffix" from "explicit i32/isize" (`crates/glyim-frontend/src/parser/expr.rs` number parsing + `crates/glyim-hir` `Literal::Int(u128, Option<IntTy>)` — introduce `Option<Option<IntTy>>` or a `None`-sentinel `IntTy::Unsuffixed`), then here:
```diff
-        Literal::Int(_, Some(IntTy::I32))
-        | Literal::Int(_, Some(IntTy::Isize))
-        | Literal::Int(_, None) => {
+        Literal::Int(_, None) => {
             let var = infer.new_int_var(ctx);
             ctx.mk_ty(TyKind::Infer(InferVar::Int(var)))
         }
+        Literal::Int(v, Some(ty)) => ctx.mk_ty(TyKind::Int(ty)), // concrete
```
(And in the parser, stop tagging unsuffixed literals as `Some(I32)` — emit `None`.)

<a name="solve-15"></a>
### [SOLVE-15] [M] `fully_resolve` reports `Ok` while unresolved int/float vars remain nested in the type
**Where:** `crates/glyim-solve/src/infer.rs:1240-1277` (skip at 1251), `1199-1207`
`collect_unresolved_vars` skips `Infer(Int/Float)` entirely, so `fully_resolve(Adt(X, [Infer(Int v)]))` with unbound `v` returns `Ok` with an infer type inside; only the later zonk masks it by defaulting to `i32`.

**Fix:**
```diff
-            TyKind::Infer(InferVar::Int(_)) | TyKind::Infer(InferVar::Float(_)) => {}
+            TyKind::Infer(InferVar::Int(v)) => {
+                if let Some(val) = self.int_vars.get(*v).and_then(|x| x.value) {
+                    self.collect_inner(ctx, val, unresolved, seen);   // follow the chain
+                } else {
+                    unresolved.push(ty);                              // genuinely unresolved
+                }
+            }
```
(mirror for Float; use a visited set — see SOLVE-3.)

<a name="solve-16"></a>
### [SOLVE-16] [M] `Never` arm precedes the Infer arms — `unify(?X, !)` leaves `?X` unbound
**Where:** `crates/glyim-solve/src/infer.rs:226-227`
Diverging expressions (`return`, `loop {}`, `panic!()`) should pin the peer var to `!`/the other side; instead unification silently succeeds with no binding.

**Fix:** move the `Never` arms below the `Infer` arms (after :284), or add an explicit case:
```diff
+            (TyKind::Infer(InferVar::Ty(v)), TyKind::Never)
+            | (TyKind::Never, TyKind::Infer(InferVar::Ty(v))) => {
+                self.ty_vars[*v].value = Some(other_side); // bind to Never
+                Ok(Vec::new())
+            }
```
**Check:** `let x = if c { return; } else { 1i64 };` types `x` as `i64`, not "annotations needed".

<a name="solve-17"></a>
### [SOLVE-17] [M] `Param` unification compares only the index — cross-item params unify
**Where:** `crates/glyim-solve/src/infer.rs:388-399`
`fn f<A>()`'s `A` and `struct S<B>`'s `B` are both `Param(0)` and compare equal; `name` is ignored too. Projection paths that keep raw params (`unify.rs:1027-1066`) expose it.

**Fix:**
```diff
-                if param_a.index == param_b.index {
+                if param_a.index == param_b.index && param_a.name == param_b.name {
```
(Longer term: add a `def_id: DefId` to `ParamTy` in `crates/glyim-type/src/ty.rs:165-172` and compare it.)

<a name="solve-18"></a>
### [SOLVE-18] [M] Orphan-rule check is `!substs.is_empty()` — vacuous
**Where:** `crates/glyim-solve/src/solver.rs:183-186`
Any impl with ≥1 substitution argument passes; `impl Display for i32` (foreign trait + foreign primitive) is accepted.

**Fix:**
```diff
     fn substs_has_local_type(&self, substs: &Substitution) -> bool {
-        !substs.is_empty()
+        substs.arguments().any(|arg| match arg {
+            GenericArg::Ty(ty) => matches!(self.ctx.ty_kind(*ty),
+                TyKind::Adt(id, _) if self.is_crate_local_adt(*id)),  // impl below builtin range
+            _ => false,
+        })
     }
```
(Add a small `is_crate_local_adt` helper comparing the AdtId against the crate's registered ADTs in `def_map`.)

<a name="solve-19"></a>
### [SOLVE-19] [M] HRTB substitution leaves `Dynamic` predicate binders unsubstituted
**Where:** `crates/glyim-solve/src/hrtb.rs:175-178`
The outer existential region is substituted but `preds: Binder<Box<[Predicate]>>` passes through untouched — late-bound regions inside dyn predicates survive instantiation.

**Fix:**
```diff
             TyKind::Dynamic(preds, region) => {
                 let region = region.substitute(sub, ctx);
-                ctx.mk_ty(TyKind::Dynamic(preds, region))
+                let new_preds = preds.value.substitute(sub, ctx);
+                ctx.mk_ty(TyKind::Dynamic(Binder::bind(new_preds, preds.bound_vars.clone()), region))
             }
```
(Use the crate's actual `Binder` API; `Predicate::substitute` at hrtb.rs:206-240 is the reference implementation.)

<a name="solve-20"></a>
### [SOLVE-20] [P] `check_path` re-resolves *every* impl's self_ty per path lookup — O(paths × items), mints dead infer vars
**Where:** `crates/glyim-typeck/src/unify.rs:609-708` (same pattern at check_expr.rs:2993-3008)
For every `Type::method` path, the code walks all HIR items calling `resolve_type_ref` per inherent impl; each resolution for generic impls allocates fresh inference vars that pile up in the table (inflating SOLVE-21's cost).

**Fix:** build the map once per body (at the start of `FnCtxt::check_body`):
```rust
// Pre-pass: resolve every inherent impl's Self exactly once.
let mut inherent_by_adt: HashMap<AdtId, Vec<(usize /*item idx*/, Ty /*self ty*/)>> = HashMap::new();
for (idx, item) in self.hir.items.iter_enumerated() {
    if let ItemKind::Impl(imp) = &item.kind && imp.trait_ref.is_none() {
        let mut diags = Vec::new();
        if let Some(ty) = resolve_type_ref_quiet(self.ctx, self.infer, self.def_map, &imp.self_ty, &mut diags) {
            if let TyKind::Adt(id, _) = self.ctx.ty_kind(ty) {
                inherent_by_adt.entry(*id).or_default().push((idx, ty));
            }
        }
    }
}
```
then `check_path` looks up `inherent_by_adt.get(&adt_id)` instead of scanning. Reuse the same map for the trait-method fallback in check_expr.rs.

<a name="solve-21"></a>
### [SOLVE-21] [P] `InferenceTable::snapshot` deep-clones all four var tables — used per method-candidate probe
**Where:** `crates/glyim-solve/src/infer.rs:77-95` (callers: check_expr.rs:2837, :3010)
With V vars alive and C candidates per call, that's O(V·C) copying per call and O(V·C·calls) overall.

**Fix:** add an undo log:
```rust
enum UndoEntry { TyVar(u32, Option<Ty>, VariableKind), IntVar(u32, Option<Ty>), FloatVar(u32, Option<Ty>), RegionVar(u32, Option<Region>) }

pub struct InferenceTable {
    // existing fields...
    undo_log: Vec<UndoEntry>,
    snapshots: Vec<usize>,   // undo_log lengths
}
```
- Every write site (there are 4: `ty_vars[var].value = …` in the general arm, `int_vars[var].value = …`, float arm, region arm) first pushes the old value onto `undo_log`.
- `snapshot()` → `self.snapshots.push(self.undo_log.len())` (O(1)).
- `rollback_to()` → pop the marker; truncate the log; replay entries backwards restoring old values.
- `commit()` → drop the marker (leave entries; they're only needed for rollback).

**Check:** `cargo test -p glyim-solve -p glyim-typeck` all green — snapshot semantics unchanged, just O(writes-since-mark).

<a name="solve-22"></a>
### [SOLVE-22] [P] Linear scans + `std::env::var` syscalls in solver hot paths
**Where:** `crates/glyim-solve/src/solver.rs:132-136, 148-153`; `crates/glyim-typeck/src/unify.rs:918-920, 937-944`
`impls_of_trait` scans *all* impls per goal (called recursively per impl predicate — see SOLVE-8); `trait_name` scans all traits; `variant_expr` linearly scans a HashMap `.iter().find(...)` per enum-variant construction; and `std::env::var("GLYIM_DBG_VEXPR")` (a lock + linear env scan) executes per variant expression in release builds.

**Fix:**
```diff
 pub struct TraitSolver {
     pub impl_defs: Vec<ImplDef>,
+    impls_by_trait: HashMap<TraitDefId, Vec<usize>>,
+    trait_names: HashMap<TraitDefId, Name>,
 }
 // populate both in register_trait / register_impl
     pub fn impls_of_trait(&self, trait_id: TraitDefId) -> impl Iterator<Item = &ImplDef> {
-        self.impl_defs.iter().filter(move |i| i.trait_ref.def_id == trait_id)
+        self.impls_by_trait.get(&trait_id).into_iter().flatten()
+            .map(move |&i| &self.impl_defs[i])
     }
```
For the env vars, add `static DBG_VEXPR: OnceLock<bool> = OnceLock::new();` and read it once. In unify.rs, resolve the ctor `FnDefId` once per (adt, variant) and cache it on `TyCtx`.

<a name="solve-23"></a>
### [SOLVE-23] [P] `unify_tys`/`occurs` have no recursion budget (deep types overflow the stack)
**Where:** `crates/glyim-solve/src/infer.rs:210-1077, 135-169, 1240-1277`
HRTB code sets the convention `MAX_STRUCT_EQ_DEPTH = 256` (hrtb.rs:259-277) but structural unification recurses unbounded over nested Adt/Tuple/FnPtr. `occurs` also calls `resolve_ty_shallow` (fresh HashSet per node!) once per visited node.

**Fix:** thread `depth: u32` through `unify_tys`, `occurs`, and `collect_unresolved_vars`; return a "types too deep" error past 256. For `occurs`, hoist resolution to the top-level caller and pass the result down, instead of re-resolving per node.

---

## 3. `glyim-mir` / `glyim-opt` / `glyim-mir-interp` / `glyim-borrowck` (31 findings)

The optimizer and interpreter. This crate cluster contains the most dangerous miscompiles in the repo.

<a name="mir-1"></a>
### [MIR-1] [C] Const-prop rewrite phase substitutes **block-exit** constants into **every** statement — silent wrong values
**Where:** `crates/glyim-opt/src/constant_prop.rs:442-451`
`in_maps[bb_idx]` holds the block's **exit** state (assigned at :436 after all statements). The rewrite applies that one map to all statements, never simulating the block. Given
```text
_1 = const 10
_2 = move _1      // must be 10
_1 = const 3
_3 = move _1      // 3
```
the exit map `{_1: 3, _2: 10, _3: 3}` rewrites `_2 = move _1` → `_2 = const 3`. **The program computes 3 where it should compute 10** for any straight-line `read x; x = other-const` sequence.

**Fix:** simulate the block statement-by-statement starting from the *entry* map:
```diff
     for bb_idx in 0..num_blocks {
-        if let Some(map) = &in_maps[bb_idx] {
-            let block = &mut body.basic_blocks[BasicBlockIdx::from_raw(bb_idx as u32)];
-            for stmt in &mut block.statements {
-                if let StatementKind::Assign(_place, rvalue) = &mut stmt.kind {
-                    replace_in_rvalue(rvalue, map);
-                }
-            }
+        // in_maps currently stores the EXIT state; reconstruct the entry state
+        // by running the transfer function backwards is wrong — instead store
+        // ENTRY maps during the fixpoint (add a parallel `entry_maps` vec
+        // populated with `incoming` before the transfer runs), then:
+        if let Some(map) = entry_maps[bb_idx].clone() {
+            let block = &mut body.basic_blocks[BasicBlockIdx::from_raw(bb_idx as u32)];
+            let mut map = map;
+            for stmt in &mut block.statements {
+                if let StatementKind::Assign(place, rvalue) = &mut stmt.kind {
+                    replace_in_rvalue(rvalue, &map);
+                    // apply the same transfer used in the fixpoint:
+                    let evaluated = evaluate_rvalue_to_const(rvalue, &map, /*locals*/);
+                    map.insert(place.local, evaluated);
+                }
+            }
         }
     }
```
Concretely: in the fixpoint loop, after computing `incoming` (line ~388) also store `entry_maps[bb_idx] = Some(incoming.clone())` before the out-state transfer. That is the whole change.

**Check:** new opt test with the 4-statement body above — `_2` must fold to `const 10`.

<a name="mir-2"></a>
### [MIR-2] [H] Const-prop folds Add/Sub/Mul with raw i128 arithmetic — overflow panics the compiler (debug) or silently wraps (release)
**Where:** `crates/glyim-opt/src/constant_prop.rs:106-149`
(a) Debug-build ICE on `i128::MAX + 1`. (b) Release silently wraps and **deletes the runtime overflow assert** the language promises. (c) The result is never range-checked against the destination type: `_2: i32 = 2147483647 + 1` folds to `Int(2147483648)` typed `i32`.

**Fix:**
```diff
                 (MirConstKind::Int(l), MirConstKind::Int(r)) => {
                     let result = match op {
-                        BinOp::Add => l + r,
-                        BinOp::Sub => l - r,
-                        BinOp::Mul => l * r,
+                        BinOp::Add => l.checked_add(*r)?,
+                        BinOp::Sub => l.checked_sub(*r)?,
+                        BinOp::Mul => l.checked_mul(*r)?,
                         …
                     };
+                    // Range-check against the operand type before folding:
+                    if !fits_ty(result, &left.ty) { return None; }   // keep runtime Assert
```
Add a `fn fits_ty(v: i128, ty: &Ty) -> bool` mapping `Int(I8) => i8::MIN as i128..=i8::MAX as i128`, etc. (mirror for the Uint arm with `checked_*` on u128).

**Check:** fold `_2: i8 = 100 + 100` must NOT fold (keeps the overflow assert); `100 + 27` folds to 127.

<a name="mir-3"></a>
### [MIR-3] [H] Const-prop folds float `x / 0.0` to `0.0` — contradicts IEEE, the interpreter, and the LLVM backend
**Where:** `crates/glyim-opt/src/constant_prop.rs:171-177`
Rust semantics: `1.0/0.0 = +inf`, `0.0/0.0 = NaN`. The interpreter (mir-interp lib.rs:938-941) instead panics "division by zero"; LLVM produces `inf`. Three behaviors for one program depending on evaluation path.

**Fix:**
```diff
                         BinOp::Div => {
-                            if rf != 0.0 { lf / rf } else { 0.0 }
+                            lf / rf   // IEEE: inf / NaN; LLVM agrees
                         }
```
AND align the interpreter: in `crates/glyim-mir-interp/src/lib.rs:938-941` remove the float div-by-zero panic (keep it for integer division only).

**Check:** `const X: f64 = 1.0 / 0.0;` interp == LLVM == const-folded (inf).

<a name="mir-4"></a>
### [MIR-4] [H] Const-prop folds float comparisons into a `FloatBits` constant typed as float instead of `Bool`
**Where:** `crates/glyim-opt/src/constant_prop.rs:178-226`
`_2: bool = _1 < _3` (floats) folds to `const FloatBits(1.0): f64` — the interpreter's Assert then fails "assert condition must be bool" (interp lib.rs:498-507) or the backend materializes an 8-byte float into a 1-byte bool slot.

**Fix:**
```diff
                         BinOp::Eq => { if lf == rf { 1.0 } else { 0.0 } }
                         …
-                    Some(MirConst { kind: MirConstKind::FloatBits(result.to_bits()), ty: left.ty, span: left.span })
+                    // comparisons produce bool, typed bool:
+                    let is_cmp = matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq);
+                    if is_cmp {
+                        Some(MirConst { kind: MirConstKind::Bool(result != 0.0), ty: /* bool ty from ctx */, span: left.span })
+                    } else {
+                        Some(MirConst { kind: MirConstKind::FloatBits(result.to_bits()), ty: left.ty, span: left.span })
+                    }
```
While here, also fold integer comparisons (Int/Uint Eq/Ne/Lt/…), which currently fall through to `None`.

**Check:** fold `_2: bool = 1.0 < 2.0` → `const Bool(true)`.

<a name="mir-5"></a>
### [MIR-5] [H] Const-prop "folds" `Cast` by returning the operand unchanged — wrong kind AND wrong type
**Where:** `crates/glyim-opt/src/constant_prop.rs:319`
`_2: u8 = _1 as u8` with `_1 = const -1 (i32)` folds to `Int(-1)` typed `i32`.

**Fix (safest):**
```diff
-        Rvalue::Cast(_, op, _) => operand_to_const(op, locals),
+        // Don't fold casts: the conversion semantics (trunc/zext/sat) belong to
+        // the backend, and propagating the wrong kind/ty poisons consumers.
+        Rvalue::Cast(_, _, _) => None,
```
**Check:** the constant above stays a runtime `Cast`; interp/codegen unaffected.

<a name="mir-6"></a>
### [MIR-6] [C] DCE doesn't count `Drop` terminators as uses — deletes the only initialization of dropped locals
**Where:** `crates/glyim-opt/src/dce.rs:77-99`
`TerminatorKind::Drop { place, .. }` matches the `_ => {}` arm, so `place.local` is never marked used. Lowering emits scope-end `Drop` terminators (`glyim-lower/src/builder.rs:249-258`), and they're present when `optimize()` runs. Result: DCE deletes `_5 = move _4` (local "unused"), leaving `Drop { _5 }` to drop an **uninitialized** local — uninitialized-memory UB / double-free path.

**Fix:**
```diff
         TerminatorKind::Assert { cond, .. } => {
             collect_operand_uses(cond, used);
         }
+        TerminatorKind::Drop { place, .. } => {
+            used.insert(place.local);
+        }
+        TerminatorKind::Call { destination, .. } => {
+            // A call may have side effects; keep its destination slot alive too.
+            used.insert(destination.local);
+        }
         _ => {}
```
**Check:** new DCE test with `_5 = move _4; Drop(_5);` — the Assign must survive.

<a name="mir-7"></a>
### [MIR-7] [H] `remap_terminator` silently retargets unknown/deleted blocks to block 0
**Where:** `crates/glyim-opt/src/cfg_simplify.rs:105-113`
Any dangling edge becomes a silent **jump to function entry** — can turn a Return path into an infinite loop. This helper is reused by drop_elaboration (where it actively corrupts the CFG, see MIR-10) and unreachable_elim.

**Fix:**
```diff
     let map = |idx: &BasicBlockIdx| -> BasicBlockIdx {
         let old = idx.to_raw() as usize;
         if let Some(&Some(new)) = remap.get(old) {
             BasicBlockIdx::from_raw(new as u32)
         } else {
-            BasicBlockIdx::from_raw(0)
+            // Never silently redirect; a dangling edge is an internal bug.
+            debug_assert!(false, "remap_terminator: dangling target {old:?}");
+            BasicBlockIdx::from_raw(old as u32) // keep original; surface loudly
         }
     };
```
Also refuse to merge blocks whose `is_cleanup` flags differ (cfg_simplify.rs:25-42 never checks it).

<a name="mir-8"></a>
### [MIR-8] [P] `unreachable_elim::reachable_set` uses `HashSet<usize>` + unvalidated successor pushes
**Where:** `crates/glyim-opt/src/unreachable_elim.rs:32-43`
Dense sets want `FixedBitSet` (borrowck already uses it); `blocks[i]` is indexed without bounds check on pushed successors.

**Fix:**
```diff
 fn reachable_set(blocks: &[BasicBlockData]) -> FixedBitSet {
-    let mut visited = HashSet::new();
+    let mut visited = FixedBitSet::with_capacity(blocks.len());
     let mut stack = vec![0usize];
     while let Some(i) = stack.pop() {
-        if visited.insert(i) {
+        if i < blocks.len() && !visited.contains(i) {
+            visited.insert(i);
             for succ in super::cfg_simplify::terminator_successors(&blocks[i].terminator) {
-                stack.push(succ.to_raw() as usize);
+                let si = succ.to_raw() as usize;
+                if si < blocks.len() { stack.push(si); }
             }
         }
     }
     visited
 }
```

<a name="mir-9"></a>
### [MIR-9] [P] Const-prop fixpoint is a full-sweep loop with per-block map clones — no worklist
**Where:** `crates/glyim-opt/src/constant_prop.rs:381-440`
O(iterations × blocks × locals) with heap churn per merge, capped only by `MAX_ITERATIONS = 1000`.

**Fix:** switch to a worklist seeded with block 0; re-enqueue successors only when a block's out-state changes (the borrowck crate's `liveness.rs` already demonstrates this pattern — copy its structure). Replace `HashMap<LocalIdx, Option<…>>` with `Vec<Option<ConstValue>>` sized to locals.

<a name="mir-10"></a>
### [MIR-10] [C] `drop_elaboration::run` corrupts the CFG whenever it creates new blocks
**Where:** `crates/glyim-opt/src/drop_elaboration.rs:335-452` (esp. :336, :438-448)
`block_map` is sized to the OLD block count and only contains old→new entries. New blocks emitted by `emit_array_drop_loop` (:477-600) and the drop-flag guard (:387-407) push terminators with **final** indices; then `remap_terminator` is applied to **all** new blocks: edges < N get wrongly remapped; edges ≥ N hit the `None` fallback → retargeted to block 0 (MIR-7). Verified trace: the array-drop init `Goto{init}` becomes a **self-loop** — the array never drops and the function hangs until any step limit.

The unit test `array_drop_creates_loop` passes only because it greps for a `SwitchInt` and never checks edges.

**Fix:**
```diff
-    for block in &mut new_blocks {
-        super::cfg_simplify::remap_terminator(block, &block_map);
-    }
+    // Only the ORIGINAL blocks' terminators reference old indices. The appended
+    // blocks (array-drop loop, flag guards) were built with final indices and
+    // must NOT be remapped.
+    let old_count = body.basic_blocks.len();
+    for block in new_blocks.iter_mut().take(old_count) {
+        super::cfg_simplify::remap_terminator(block, &block_map);
+    }
```
Wait — that's still wrong because appended blocks are *interleaved* before old ones. The correct minimal fix is to make `block_map` identity-mapped for appended blocks and remap everything:
```diff
-    let mut block_map: Vec<Option<usize>> = vec![None; body.basic_blocks.len()];
+    // sized later, after new_blocks is complete
     …
     for (old_idx, old_block) in body.basic_blocks.iter().enumerate() {
         let new_idx = new_blocks.len();
-        block_map[old_idx] = Some(new_idx);
         new_blocks.push(…);
     }
+    // Build the map only now, when every appended block has a final index:
+    let mut block_map: Vec<Option<usize>> = vec![None; body.basic_blocks.len()];
+    for (i, nb) in new_blocks.iter().enumerate() { /* identity for appended */ }
```
**Cleanest correct restructure (follow this):**
1. First pass: for each old block, compute `new_idx = new_blocks.len()` BEFORE pushing, and record `block_map[old_idx] = Some(new_idx)`; push the old block with its terminator as-is. Appended blocks (loop/guards) are NOT pushed yet.
2. Second pass: remap terminators of the pushed old blocks only.
3. Third pass: emit the appended blocks (loop init/cond/body/inc, flag guards) — they already reference final indices — pushing them at the end and patching the old blocks' `Drop` terminators to `Goto { target: init_block_final }` where `init_block_final = new_blocks.len()` is now known.
4. Add a regression test asserting the exact edge structure (e.g. init→cond, cond→body/exit, body→inc, inc→cond), not just terminator kinds.

**Check:** interpret a body with `[String; 3]` and a scope-end Drop: all 3 elements drop (use the interpreter's drop counting), no hang.

<a name="mir-11"></a>
### [MIR-11] [C] `MaybeInitialized` is a MAY (union) analysis but consumed as definite initialization
**Where:** `crates/glyim-opt/src/drop_elaboration.rs:33-81` (analysis), :382-426 (consumption)
The dataflow unions over predecessors (possibly-initialized) but `is_definitely_initialized` (:384) decides whether a `Drop` runs unguarded. `if c { _5 = make_string(); }` merging → `entry[merge][_5] = true` → unconditional drop → on the `!c` path it drops an **uninitialized** local. Two aggravators: `StorageLive` marks initialized (:51-53), and when no flag local exists the code emits an *unconditional* Drop anyway (:413-419), contradicting the module doc.

**Fix:** compute the dual **must**-analysis:
```diff
-    // union over predecessors:
-    for i in 0..num_locals { if cur[i] && !succ_entry[i] { succ_entry[i] = true; changed_succ = true; } }
+    // must-analysis: intersect over REACHABLE predecessors; init only via Assign.
```
Concretely: (1) initialize every non-entry block's entry state to all-`false`; (2) iterate: `succ_entry[i] &= cur[i]` for reachable predecessors only (track reachability with a bitset); (3) the transfer function sets init only on `Assign(place, _)` — **remove** the `StorageLive` case (:51-53) — and clears on `Drop`/move-out; (4) at :413-419, when not definitely initialized and no flag local exists, keep the drop but route it through the flag path or emit a diagnostic, never an unconditional drop.

**Check:** `if c { s = String::new(); }` then scope end — the drop must be guarded; interp drop count is 1 when `c == true`, 0 otherwise.

<a name="mir-12"></a>
### [MIR-12] [M] Array-drop loop drops `p[i]` instead of `(*p)[i]` — loses the leading `Deref`
**Where:** `crates/glyim-opt/src/drop_elaboration.rs:537-540`
`elem_place` hardcodes projection `[Index(idx)]`, dropping the original `Deref`/`Field` prefix.

**Fix:**
```diff
     let elem_place = Place {
         local: place.local,
-        projection: vec![ProjectionElem::Index(idx_local)].into_boxed_slice(),
+        projection: {
+            let mut proj: Vec<_> = place.projection[..place.projection.len() - 1].to_vec();
+            proj.push(ProjectionElem::Index(idx_local));
+            proj.into_boxed_slice()
+        },
     };
```
**Check:** drop an array behind `&mut` — elements' projections contain `Deref, Index`.

<a name="mir-13"></a>
### [MIR-13] [C] Interpreter arithmetic is unbounded 128-bit wrapping — `i8`/`i32` arithmetic disagrees with every compiled binary
**Where:** `crates/glyim-mir-interp/src/lib.rs:839-907`
`InterpValue::Int(i128)` never truncates to the operand's declared width; `100i8 + 100i8` yields `200` in the interpreter and `-56` (or an overflow panic) in an LLVM binary.

**Fix:** thread the destination scalar type into `eval_binary_op` (it's available from the rvalue's target local decl) and truncate per type after each op:
```rust
fn trunc_to_ty(v: i128, ty: Ty, ctx: &dyn TypeLookup) -> i128 {
    match ctx.ty_kind(ty) {
        TyKind::Int(IntTy::I8) => v as i8 as i128,
        TyKind::Int(IntTy::I16) => v as i16 as i128,
        TyKind::Int(IntTy::I32) => v as i32 as i128,
        TyKind::Int(IntTy::I64) => v as i64 as i128,
        TyKind::Uint(IntTy::U8) => (v as u8) as i128,
        // … U16/U32/U64/Usize …
        _ => v,
    }
}
```
and apply: `let result = trunc_to_ty(raw_result, dest_ty, ctx);` in both the Int and Uint arms. For overflow-checking semantics, compare `raw_result` against the type range and return `InterpError::Panic("attempt to add with overflow")` when out of range and `overflow_checks` are on (add a flag mirroring the CLI's opt-level).

**Check:** interp run of `100i8 + 100i8` == `-56` == LLVM output.

<a name="mir-14"></a>
### [MIR-14] [C] `CastKind::IntToInt` is an identity no-op in the interpreter — `(-1i32) as u8` gives -1, not 255
**Where:** `crates/glyim-mir-interp/src/lib.rs:725-747`
The LLVM backend does real trunc/zext (lower.rs:2294-2328); the interpreter returns the value unchanged. Every cast-bearing program diverges between interp and binary.

**Fix:**
```diff
-                    CastKind::IntToInt => Ok(val),
+                    CastKind::IntToInt => {
+                        let target = _target_ty; // currently ignored — use it
+                        match val {
+                            InterpValue::Int(v) => Ok(InterpValue::Int(trunc_to_ty(v, target, ctx))),
+                            InterpValue::Uint(v) => Ok(InterpValue::Uint(trunc_u_to_ty(v, target, ctx))),
+                            other => Ok(other),
+                        }
+                    }
```
(also apply real width conversion to `FloatToInt` — `v as i128` → truncate to target width — instead of the raw cast at :741.)

**Check:** interp `let x: i32 = -1; let y = x as u8;` yields 255.

<a name="mir-15"></a>
### [MIR-15] [M] Host panic (index out of bounds) after `Call { target: None }` in the last block
**Where:** `crates/glyim-mir-interp/src/lib.rs:458-459, 293`
A diverging call's fabricated fall-through `bb_idx + 1` can equal `blocks.len()`; the next loop iteration indexes `body.basic_blocks[bb_idx]` → Rust index panic (ICE), not a typed error.

**Fix:**
```diff
                     let next_bb = target
                         .unwrap_or_else(|| BasicBlockIdx::from_raw((bb_idx.index() + 1) as u32));
+                    if next_bb.index() as usize >= body.basic_blocks.len() {
+                        return Err(InterpError::Panic(
+                            "call with no target fell off the end of the function".into()));
+                    }
```
**Check:** a function whose last block is a `target: None` call interprets to a clean `InterpError`, not a crash.

<a name="mir-16"></a>
### [MIR-16] [H] `Rvalue::Ref` discards the projection — `&x.f` borrows all of `x`
**Where:** `crates/glyim-mir-interp/src/lib.rs:647-653`
`InterpValue::Ref { frame, local }` has no room for projections; writes through such a ref overwrite the whole base.

**Fix (minimal honest):** reject unsupported borrows loudly instead of corrupting:
```diff
             Rvalue::Ref(place, _borrow_kind) => {
+                if !place.projection.is_empty() {
+                    return Err(InterpError::Unsupported(
+                        "borrowing a projected place is not yet modeled by the interpreter".into()));
+                }
                 let local_idx = place.local.index();
                 Ok(InterpValue::Ref { frame: self.frame_depth, local: local_idx })
             }
```
**Fix (complete):** extend `InterpValue::Ref` with `proj: Box<[ProjectionElem]>` and apply it in `read_place`/`write_place_frame` after the initial deref.

**Check:** existing tests green; `&x.f` either works correctly or errors clearly (never corrupts).

<a name="mir-17"></a>
### [MIR-17] [C] Interpreter enum field **writes** skip the tag adjustment that reads do — writes corrupt the discriminant
**Where:** `crates/glyim-mir-interp/src/lib.rs:1414-1416` (Downcast write), `1358-1377` (Field write); read path at :1064-1080
Enums store as `[tag, …payload]`. The read path offsets `Field(fi)` by `+1` when the base is an enum (:1073) and `Downcast` strips the tag (:1141-1148). The write path does neither: `((e as B).0) = v` executes `fields[0] = v` — **overwrites the variant tag with payload data**; a subsequent match dispatches to a garbage variant.

**Fix:** thread the base's `Ty` through `write_through_projections_with_locals` (exactly like the read path's `current_ty`) and mirror the read path:
```diff
-            ProjectionElem::Downcast(_) => {
-                Ok(self.write_through_projections_with_locals(base, rest, val)?)
-            }
+            ProjectionElem::Downcast(variant) => {
+                // strip the tag: payloads live past element 0
+                let payload = match base {
+                    InterpValue::Aggregate(fields) if is_enum_ty(current_ty, ctx) => {
+                        InterpValue::Aggregate(fields[1..].to_vec())
+                    }
+                    other => other,
+                };
+                let written = self.write_through_projections_with_locals(payload, rest, val)?;
+                // reassemble [tag, …payload] with the (possibly new) tag:
+                …
+            }
```
and in the `Field(fi)` write arm apply the same `let adjusted = if is_enum { fi + 1 } else { fi };` used at :1073.

**Check:** interp program writing an enum payload then matching on the value dispatches to the right variant with the right payload.

<a name="mir-18"></a>
### [MIR-18] [H] Subslice pointer arithmetic adds element offsets to a *local index* — refs point into arbitrary locals
**Where:** `crates/glyim-mir-interp/src/lib.rs:1225-1240`
`Ref { local }` identifies a local slot, not an address; `local + start * elem_size` produces a "pointer" to whichever unrelated local sits at that index, and the non-ref fallback fabricates `Ref { frame: 0, local: 0 }` (aliases frame 0's return slot).

**Fix (minimal):** represent subslices as owned element aggregates:
```diff
-                    let data_ptr = if let InterpValue::Ref { frame, local } = val {
-                        let ptr = local + start * elem_size;
-                        InterpValue::Ref { frame, local: ptr }
-                    } else {
-                        InterpValue::Ref { frame: 0, local: 0 }
-                    };
-                    val = InterpValue::Aggregate(vec![data_ptr, InterpValue::Int(new_len as i128)]);
+                    // Copy the elements [start..start+new_len] by value instead of
+                    // fabricating a bogus pointer:
+                    let elems = match &val {
+                        InterpValue::Aggregate(fields) => fields[start..start + new_len].to_vec(),
+                        other => return Err(InterpError::Unsupported(format!(
+                            "subslice of {:?}", other))),
+                    };
+                    val = InterpValue::Aggregate(elems);
```
**Check:** `let s = &arr[1..3]; s[0] == arr[1]` in the interpreter.

<a name="mir-19"></a>
### [MIR-19] [P] Interpreter deep-clones the entire callee `Body` on **every call**
**Where:** `crates/glyim-mir-interp/src/lib.rs:270, 433-439`
`function_table: HashMap<DefId, Body>` stores owned bodies; recursion performs N full-body clones at depth N. The harness (glyim-test/src/harness/interpreter_runner.rs:112) compounds it.

**Fix:**
```diff
-    function_table: HashMap<DefId, Body>,
-    current_body: Option<Body>,
+    function_table: HashMap<DefId, std::rc::Rc<Body>>,
+    current_body: Option<std::rc::Rc<Body>>,
```
- `add_function` stores `Rc::new(body)`.
- `:433` becomes `let callee_body = self.function_table.get(&callee_id).cloned()` — now an `Rc` bump, O(1).
- Anywhere the code needs `&mut Body` (it shouldn't — bodies are read-only during interp), clone explicitly at that one site.
**Check:** run the deep-recursion stress test (`tests/stress.rs`) — runtime drops dramatically.

<a name="mir-20"></a>
### [MIR-20] [M] `debug_assert!(false, …)` fires on every un-elaborated `Drop` — debug runs abort on legitimate MIR
**Where:** `crates/glyim-mir-interp/src/lib.rs:536-546`
`glyim_opt::optimize()` doesn't run drop elaboration, and the codegen pipeline also feeds Drop-carrying bodies onward, so debug interp runs panic on any droppable local whose scope-end Drop survived; release silently no-ops the same body — behavior differs by build profile.

**Fix:**
```diff
-                    debug_assert!(false, "Drop terminator reached the interpreter …");
+                    // Only an error if the type actually needs drop glue.
+                    debug_assert!(
+                        !self.needs_drop(place_ty),
+                        "Drop terminator reached the interpreter for droppable place {place:?}; \
+                         run drop elaboration before interpreting."
+                    );
+                    // Non-debug / non-droppable: dropping is a no-op.
```
Add a `needs_drop(ty)` helper on the interpreter using its `tcx` (layout/`TyKind`-based). Also make the pipeline run `elaborate_drops` before interpreter/codegen consumption (glyim-pipeline/src/lib.rs:519).

<a name="mir-21"></a>
### [MIR-21] [C] `places_conflict` treats `Index(i)` vs `Index(j)` with different locals as disjoint — unsound
**Where:** `crates/glyim-borrowck/src/visitor.rs:167-177`
Different index locals can hold the **same value** at runtime. `let i = 0; let j = i; let r = &mut arr[i]; arr[j] += 1; use(r);` — the write is not flagged; the mutable borrow is violated.

**Fix:**
```diff
             (ProjectionElem::Index(l1), ProjectionElem::Index(l2)) => {
-                if l1 == l2 {
-                    continue;
-                } else {
-                    return false;
-                }
+                // Runtime index values are unknown here: two Index projections on
+                // the same base MAY alias. Be conservative.
+                continue;
             }
```
**Check:** new borrowck test — the scenario above must produce "cannot assign … while borrowed".

<a name="mir-22"></a>
### [MIR-22] [H] `Drop` terminators bypass all conflict checks — dropping a borrowed value is accepted
**Where:** `crates/glyim-borrowck/src/visitor.rs:69-71` + `crates/glyim-borrowck/src/lib.rs:180-225`
`check_terminator_conflicts` reuses `walk_terminator_reads`, which skips Drop entirely. `drop(s)` while `s` is borrowed → no diagnostic → freed-while-aliased.

**Fix:** in `check_terminator_conflicts` (lib.rs), before the generic reads walk:
```diff
+        if let TerminatorKind::Drop { place, .. } = &term.kind {
+            // Dropping is a move-out: conflicts with ANY active loan.
+            for loan in active_loans.iter() {
+                if places_conflict(ctx, &loan.assigned_place, place) {
+                    diagnostics.push(cannot_move_out_while_borrowed(loan, place, term.span));
+                }
+            }
+            return; // no reads to check afterwards
+        }
```
**Check:** the scenario above errors.

<a name="mir-23"></a>
### [MIR-23] [M] Loan activity is purely "dest local is live" — loans never killed on reassignment, active before creation around back-edges
**Where:** `crates/glyim-borrowck/src/lib.rs:404-414`
`_3 = &mut _a; …; _3 = &mut _b; use(_3); let v = _a;` — at the read of `_a`, `_3` is still live, so the stale loan on `_a` is treated as active → false positive on legal Rust.

**Fix (cheapest correct approximation):** while replaying statements per block (the `active_loans` construction site), kill a loan when its `dest_local` is redefined:
```rust
// precompute once: reassigned_at: HashMap<LocalIdx, BitSet /*stmt idx*/>
// during replay at stmt i: for loan in active: if reassigned_at[loan.dest_local].contains(i) { skip loan }
```
i.e. a loan contributes to `active_loans` at statement i only if `dest_local` was not assigned between the `Rvalue::Ref` and i.

**Check:** the scenario above passes borrowck.

<a name="mir-24"></a>
### [MIR-24] [C] Move analysis ignores terminators — moves in `Call` args are invisible
**Where:** `crates/glyim-borrowck/src/move_analysis.rs:351-381, 465-499, 662-676`
Only the `Call` **destination** init is modeled; `Call { args: [Move(x)] }`, `Assert`, `SwitchInt`, `Drop` moves are never recorded. Every by-value argument pass — the most common move in the language — is invisible: use-after-move across call boundaries is accepted.

**Fix:**
```rust
// New fn, mirroring collect_stmt_move_effects:
fn collect_terminator_move_effects(term: &Terminator, move_paths: &mut MovePathArena,
    moved: &mut BitSet, ctx: &dyn BorrowckCtx, local_decls: &[LocalDecl]) {
    match &term.kind {
        TerminatorKind::Call { func, args, .. } => {
            collect_operand_move(func, move_paths, moved, ctx, local_decls);
            for arg in args { collect_operand_move(arg, move_paths, moved, ctx, local_decls); }
        }
        TerminatorKind::Assert { cond, .. } | TerminatorKind::SwitchInt { discr: cond, .. } =>
            collect_operand_move(cond, move_paths, moved, ctx, local_decls),
        TerminatorKind::Drop { place, .. } =>
            record_move(place, move_paths, moved, ctx, local_decls),
        _ => {}
    }
}
```
Apply it in the per-block effect loop right after the statements (move_analysis.rs:351-381), and add the matching **check** pass in `check_moves` (:662-676): walk terminator operands with the same used-places walker used for statements.

**Check:** `let s = String::new(); f(s); f(s);` errors with use-after-move.

<a name="mir-25"></a>
### [MIR-25] [H] Assignment into a moved-out place silently "resurrects" it
**Where:** `crates/glyim-borrowck/src/move_analysis.rs:602-613`
The Assign arm clears moved/dead bits for the destination **without checking whether any ancestor path is moved**. `_a = Move(_x); _x.t = 5; use(_x.t);` — accepted, reads a field of a moved struct.

**Fix:**
```diff
         StatementKind::Assign(dest, rvalue) => {
+            // Ancestor check: assigning into a place whose root/parent prefix is
+            // moved-out or dead is an error (E0382 class).
+            let mut prefix = dest.clone();
+            while let Some(parent_mp) = move_paths.parent_of(&prefix) {
+                if moved.contains(parent_mp.to_raw() as usize) || dead.contains(parent_mp.to_raw() as usize) {
+                    ctx.report_assign_into_moved(dest, /*which ancestor*/ parent_mp);
+                    // do NOT clear bits; leave state untouched for this path
+                    return; // still collect rvalue moves below in the caller
+                }
+                prefix = parent_mp_place(parent_mp);
+            }
             if let Some(mp_idx) = move_paths.find(dest) { … existing clearing … }
```
(`MovePath` already stores `parent` links — currently `#[allow(dead_code)]`.)

**Check:** the scenario errors; re-init after full move (`_x = String::new();` after `_a = Move(_x)`) still passes.

<a name="mir-26"></a>
### [MIR-26] [H] Move-path bitsets sized from a stale path count taken before the arena grows
**Where:** `crates/glyim-borrowck/src/move_analysis.rs:334-349` vs `351-381`
`num_paths` is captured, then effect collection lazily **creates** new paths (Downcast/ConstantIndex projections in Move operands, :131-145, :188-207) — `record_move` inserts indices ≥ `num_paths` into short bitsets → fixedbitset assert/panic or silently untracked moves. ICE on bodies that move enum-variant payloads.

**Fix:** pre-scan before capturing:
```diff
+    // Materialize every move path reachable from any Move operand first.
+    for block in &body.basic_blocks {
+        for stmt in &block.statements {
+            if let StatementKind::Assign(_, rv) = &stmt.kind {
+                precreate_paths_for_rvalue(rv, &mut move_paths, ctx, local_decls);
+            }
+        }
+        precreate_paths_for_terminator(&block.terminator, &mut move_paths, ctx, local_decls);
+    }
     let num_paths = move_paths.len();
```
(Or add `BitSet::ensure(n)` growth + re-sync, but pre-scan is simpler.)

**Check:** borrowck a body with `Match(x) { Move((x as Some).0), … }` — no panic, moves tracked.

<a name="mir-27"></a>
### [MIR-27] [M] Two-phase activation never scans `Call` terminators — the canonical `vec.push(vec.len())` case never activates
**Where:** `crates/glyim-borrowck/src/twophase.rs:44-69`
The reservation only ends when an **Assign rvalue** reads `dest_local`; using the reserved borrow in a call's arguments is never seen, so the loan stays "in reservation" (acting shared) past the call.

**Fix:** in `transfer`, after the statement loop:
```diff
+                    // A read of dest_local in the terminator also activates the phase.
+                    if current {
+                        if let Some(term) = &body.basic_blocks[block].terminator {
+                            let mut checker = LocalReadChecker::new(dest_local);
+                            walk_terminator_operand_reads(term, &mut checker);
+                            if checker.found() { current = false; }
+                        }
+                    }
```
**Check:** `v.push(v.len())` (two-phase) accepted; a *second* shared borrow after the call correctly rejected while the mut borrow lives.

<a name="mir-28"></a>
### [MIR-28] [M] `loans_by_dest` indexes by `dest_local` without the bounds defense liveness has
**Where:** `crates/glyim-borrowck/src/lib.rs:394-397`
Lowering can produce local indices `>= body.locals.len()` (liveness.rs:46-49 documents this — that's why `local_capacity` exists). Direct index → borrowck ICE.

**Fix:**
```diff
-    let mut loans_by_dest: Vec<SmallVec<[usize; 2]>> = vec![SmallVec::new(); body.locals.len()];
+    let cap = local_capacity(body);   // already exists in liveness.rs — make it pub(crate)
+    let mut loans_by_dest: Vec<SmallVec<[usize; 2]>> = vec![SmallVec::new(); cap];
```
and skip loans whose dest exceeds `cap` with a `debug_assert!`.

<a name="mir-29"></a>
### [MIR-29] [P] `compute_stmt_liveness` recomputes `local_capacity(body)` for every block — O(blocks × body)
**Where:** `crates/glyim-borrowck/src/liveness.rs:202-210` (called from lib.rs:402 per block)
`local_capacity` walks every statement of every block; it's body-global and constant.

**Fix:** compute once in `compute_liveness` and thread it:
```diff
 pub(crate) fn compute_stmt_liveness(
     body: &Body,
     block: BasicBlockIdx,
     live_out: &BitSet,
+    capacity: usize,
 ) -> Vec<BitSet> {
     …
-    let capacity = local_capacity(body);
```
update the caller loop to pass the precomputed value.

<a name="mir-30"></a>
### [MIR-30] [L] `Body::args` slices without a length check
**Where:** `crates/glyim-mir/src/lib.rs:716-719`
```diff
     pub fn args(&self) -> &[LocalDecl] {
-        &self.locals.as_slice()[1..1 + self.arg_count]
+        self.locals.as_slice().get(1..1 + self.arg_count).unwrap_or(&[])
     }
```
**Check:** `cargo test -p glyim-mir` green.

<a name="mir-31"></a>
### [MIR-31] [L] `validate_no_subslice` misses Subslice inside `Aggregate`/`Cast`/`Repeat` operands
**Where:** `crates/glyim-opt/src/validate.rs:245-252`
The post-slice_desugar invariant check only inspects `Ref/Discriminant/Len/Use` rvalues.

**Fix:** reuse the generic traversal (`walk_rvalue_reads` in borrowck/visitor.rs is the model) so every operand-bearing rvalue variant is visited; also extend `validate_body`'s terminality check (:98-104) to statement destinations and terminator places (Call destination/args, Drop place, SwitchInt discr, Assert cond).

---

## 4. `glyim-bytecode-vm` / `glyim-codegen` / `glyim-runtime` / `glyim-cli` / `glyim-layout` (33 findings)

<a name="rt-1"></a>
### [RT-1] [H] `resolve_target` indexes `block_offsets` without bounds check — panics on the `u32::MAX` trap sentinel
**Where:** `crates/glyim-bytecode-vm/src/lib.rs:235-241`
The codegen emits `u32::MAX` as the "trap" target for `OP_ASSERT` on ZST OOB indexing (codegen lib.rs:330-334) and `OP_CALL` with `target: None` (:1090). With a `block_offsets` table present, `block_offsets[0xFFFF_FFFF]` panics the host.

**Fix:**
```diff
-    fn resolve_target(&self, target: u32) -> usize {
+    fn resolve_target(&self, target: u32) -> ExecResult<usize> {
         if !self.block_offsets.is_empty() {
-            self.block_offsets[target as usize]
+            if target == u32::MAX {
+                return Err(VmError::AbnormalTermination);
+            }
+            self.block_offsets.get(target as usize).copied()
+                .ok_or(VmError::BadJumpTarget(target))
         } else {
-            target as usize
+            Ok(target as usize)
         }
     }
```
Update the 4 call sites to `?`. Add `VmError::BadJumpTarget(u32)` to the enum.

**Check:** run a program with a ZST OOB assert — typed error, no panic.

<a name="rt-2"></a>
### [RT-2] [M] `Opcode::Cast`/`Opcode::Assert` read operand bytes unchecked — panic on truncated code
**Where:** `crates/glyim-bytecode-vm/src/lib.rs:480-483, 516-518`
The top-of-loop bounds check covers only the opcode byte. If `Cast`/`Assert` is the last byte, `code[pc]` panics instead of `VmError::UnexpectedEndOfCode`.

**Fix:**
```diff
                 Opcode::Cast => {
-                    let _kind = code[self.frames[frame_idx].pc];
+                    let _kind = *code.get(self.frames[frame_idx].pc)
+                        .ok_or(VmError::UnexpectedEndOfCode)?;
                     self.frames[frame_idx].pc += 1;
                 }
```
(same for `Assert`'s `expected` byte).

<a name="rt-3"></a>
### [RT-3] [C] VM `Drop` expects inline `DROP u32` but the emitter writes `DROP; JUMP u32` — stream desyncs on every drop-terminated block
**Where:** `crates/glyim-bytecode-vm/src/lib.rs:548-553` vs emitter `crates/glyim-codegen/src/lib.rs:1110-1115`
The VM consumes the following `OP_JUMP` opcode byte + 3 target bytes as one u32 and jumps to garbage; in block-offset mode it panics per RT-1. **Every compiled program whose block ends in `Drop` mis-executes.**

**Fix (keep the emitter, simplify the VM):**
```diff
                 Opcode::Drop => {
                     let _addr = self.pop()?;
-                    let target = Vm::read_u32(code, &mut self.frames[frame_idx].pc)?;
-                    let off = module.functions[func].resolve_target(target)?;
-                    self.frames[frame_idx].pc = off;
+                    // The emitter writes `DROP; JUMP u32`. Let the following
+                    // OP_JUMP execute naturally — do not consume it here.
                 }
```
**Check:** end-to-end test: compile a fn with a `Drop` terminator, run on the VM, correct control flow.

<a name="rt-4"></a>
### [RT-4] [C] `OP_LEN` wire-format mismatch: emitter writes no operand, VM + peephole decoder consume 4 bytes; VM also pushes a constant 0
**Where:** emitter `crates/glyim-codegen/src/lib.rs:895-899`; VM lib.rs:527-530; decode table codegen lib.rs:683
`Rvalue::Len` (emitted by real lowering — lower_rvalue.rs:1449, :1805) writes bare `OP_LEN`; the VM eats the next opcode + 3 bytes (desync) and pushes `Int(0)`. The ZST path (:320-323) uses the *other* encoding, so the two emission sites disagree with each other too.

**Fix (standardize on `OP_LEN + u32 local`):**
```diff
 # crates/glyim-codegen/src/lib.rs:895
             Rvalue::Len(place) => {
-                self.emit_operand(bc, &Operand::Copy(place.clone()), local_tys)?;
                 bc.push(OP_LEN);
+                bc.extend_from_slice(&place.local.to_raw().to_le_bytes());
                 Ok(())
             }
```
```diff
 # crates/glyim-bytecode-vm/src/lib.rs:527
                 Opcode::Len => {
-                    let _local = Vm::read_u32(code, &mut self.frames[frame_idx].pc)?;
-                    self.stack.push(Value::Int(0));
+                    let local = Vm::read_u32(code, &mut self.frames[frame_idx].pc)?;
+                    let slot = self.load_local(local)?;   // existing helper
+                    let len = match slot {
+                        Value::Tuple(elems) => elems.len() as i64,
+                        other => return Err(VmError::TypeMismatch(format!("len of {other:?}"))),
+                    };
+                    self.stack.push(Value::Int(len));
                 }
```
(The peephole table at codegen :683 already expects 4 bytes — it now matches.)

<a name="rt-5"></a>
### [RT-5] [H] `OP_DISCRIMINANT`: emitter writes no operand, peephole decoder consumes 4 bytes — O1+ deletes an instruction per discriminant
**Where:** `crates/glyim-codegen/src/lib.rs:890-894` vs `:683`
At `--opt-level>=1` the peephole pass mis-decodes after `OP_DISCRIMINANT` (eats the next opcode + 3 bytes as a phantom operand) and re-encodes — silently deleting an instruction.

**Fix:** remove `OP_LEN | OP_DISCRIMINANT` from the `take(4)` list at :683 (after RT-4 fixes `OP_LEN` to carry a u32, keep only `OP_LEN` there and drop `OP_DISCRIMINANT`):
```diff
-        OP_LOAD_LOCAL | OP_STORE_LOCAL | OP_JUMP | OP_JUMP_IF | OP_LEN | OP_DISCRIMINANT => take(4),
+        OP_LOAD_LOCAL | OP_STORE_LOCAL | OP_JUMP | OP_JUMP_IF | OP_LEN => take(4),
```
and make the `Rvalue::Discriminant` emitter write the local (same pattern as RT-4).

<a name="rt-6"></a>
### [RT-6] [H] VM `Discriminant` returns tuple **arity**, not the enum tag — every match dispatches to one arm
**Where:** `crates/glyim-bytecode-vm/src/lib.rs:531-538`
Enum values are `[tag, fields…]`; `elems.len()` is the field count. `enum E { A(u32), B(u32) }` yields 2 for both variants.

**Fix:**
```diff
                 Opcode::Discriminant => {
                     let v = self.pop()?;
                     let discr = match &v {
-                        Value::Tuple(elems) => elems.len() as i64,
-                        Value::Int(_) => 0,
+                        Value::Tuple(elems) => elems.first()
+                            .and_then(|t| match t { Value::Int(i) => Some(*i), _ => None })
+                            .ok_or_else(|| VmError::TypeMismatch("untagged aggregate".into()))?,
+                        other => return Err(VmError::TypeMismatch(format!("discriminant of {other:?}"))),
                     };
                     self.stack.push(Value::Int(discr));
                 }
```
**Check:** match on a two-variant enum with payloads dispatches correctly on the VM.

<a name="rt-7"></a>
### [RT-7] [H] VM division/remainder by zero silently yield 0 instead of trapping
**Where:** `crates/glyim-bytecode-vm/src/lib.rs:614-616`
The peephole doc (codegen lib.rs:520-521) even claims "the runtime traps on it" — it doesn't.

**Fix:**
```diff
-            Opcode::Div => a.checked_div(b).unwrap_or(0),
-            Opcode::Rem => a.checked_rem(b).unwrap_or(0),
+            Opcode::Div => a.checked_div(b)
+                .ok_or(VmError::AbnormalTermination)?,   // div by zero / i64::MIN / -1
+            Opcode::Rem => a.checked_rem(b)
+                .ok_or(VmError::AbnormalTermination)?,
```
(`binop` must return `ExecResult<i64>`; update its one caller.)

<a name="rt-8"></a>
### [RT-8] [H] `set_mem` grows memory to an attacker-controlled (possibly wrapped) address
**Where:** `crates/glyim-bytecode-vm/src/lib.rs:442-446, 676-683`
`addr` comes off the stack as `i64 as usize`; `-1` becomes `usize::MAX`, `addr + 1` overflows (debug panic / release wrap); large values make `resize` attempt a gigantic allocation. Same for `Deref`/`Repeat` counts (:554-562).

**Fix:**
```diff
     fn set_mem(&mut self, addr: usize, v: Value) -> ExecResult<()> {
+        const MEM_CAP: usize = 1 << 20; // 1M slots — tune to the language spec
+        if addr >= MEM_CAP {
+            return Err(VmError::LocalOutOfBounds);
+        }
         let frame = self.frames.last_mut().ok_or(VmError::StackUnderflow)?;
         if addr >= frame.mem.len() {
             frame.mem.resize(addr + 1, Value::Int(0));
```
and at the `StoreField` arm (:676-683) reject negative addresses:
```diff
-                    let addr = self.pop()?.as_int() as usize;
+                    let raw = self.pop()?.as_int();
+                    if raw < 0 { return Err(VmError::LocalOutOfBounds); }
+                    let addr = raw as usize;
```
Apply the same bound to `Repeat`'s `count`.

<a name="rt-9"></a>
### [RT-9] [M] No step budget — `Jump 0` hangs the host forever
**Where:** `crates/glyim-bytecode-vm/src/lib.rs:401-420`
Only call depth is bounded (MAX_CALL_DEPTH, :300).

**Fix:** count branches only (near-zero overhead):
```diff
+    max_steps: Option<u64>,   // add to Vm + Default = Some(1 << 32)
     fn drive(&mut self, module: &Module) -> ExecResult<Value> {
+        let mut steps: u64 = 0;
         loop {
             …
+            if matches!(op, Opcode::Jump | Opcode::JumpIf | Opcode::SwitchInt | Opcode::Call | Opcode::Ret) {
+                steps += 1;
+                if self.max_steps.is_some_and(|m| steps > m) {
+                    return Err(VmError::StepLimitExceeded);
+                }
+            }
```
**Check:** a self-loop program returns `StepLimitExceeded`, not a hang.

<a name="rt-10"></a>
### [RT-10] [P] `Vm::run` clones the whole bytecode buffer per execution; `Call` allocates a fresh args Vec per call
**Where:** `crates/glyim-bytecode-vm/src/lib.rs:350-357, 589-601`

**Fix:**
```diff
     pub fn run(&mut self, chunk: &Chunk) -> ExecResult<Value> {
         let module = Module::new(
-            vec![Function::new(chunk.code.clone(), chunk.n_locals, 0)],
+            vec![Function::new(std::rc::Rc::from(&chunk.code[..]), chunk.n_locals, 0)],
             0,
         );
```
(or change `Function.code` to `Cow<[u8]>`), and store one scratch `Vec<Value>` on `Vm` reused by `OP_CALL`:
```rust
// field: scratch_args: Vec<Value>
// OP_CALL: self.scratch_args.clear(); …push…; call fn takes &self.scratch_args
```

<a name="rt-11"></a>
### [RT-11] [C] Bytecode backend emits **inverted branches for every bool switch** — every if/while/match-guard takes the wrong arm
**Where:** `crates/glyim-codegen/src/lib.rs:1035-1046`
Lowering always builds bool switches as `SwitchTargets::new([(1, THEN_BB)], ELSE_BB)` (lower_rvalue.rs:316, :386, :553, :1552; builder.rs:270). This code treats the *first branch pair* as the false target and `otherwise` as the true target — the exact opposite. `OP_JUMP_IF` is jump-if-nonzero (VM :489-496), so a true condition jumps to **else**. The codegen tests only assert `contains(OP_JUMP_IF)` and never check targets, so nothing catches it.

**Fix:**
```diff
                 if *switch_ty == Ty::BOOL {
                     self.emit_operand(bc, discr, local_tys)?;
-                    let false_target = targets
-                        .iter()
-                        .next()
-                        .map(|(_, t)| t)
-                        .unwrap_or_else(|| targets.otherwise());
-                    let true_target = targets.otherwise();
+                    // Lowering convention: branch value 1 = true-block, otherwise = false-block.
+                    let true_target = targets
+                        .iter()
+                        .find(|(v, _)| *v == 1)
+                        .map(|(_, t)| t)
+                        .unwrap_or_else(|| targets.otherwise());
+                    let false_target = targets.otherwise();
                     bc.push(OP_JUMP_IF);
                     bc.extend_from_slice(&true_target.to_raw().to_le_bytes());
                     bc.push(OP_JUMP);
                     bc.extend_from_slice(&false_target.to_raw().to_le_bytes());
```
**Check:** compile `fn main() { let x = 5; if x > 3 { exit(1); } exit(0); }` to bytecode and run — must exit 1 (currently exits 0).

<a name="rt-12"></a>
### [RT-12] [C] Emitted jump targets are block indices, but no `block_offsets` table is ever produced — VM jumps to byte offset = block index (mid-instruction)
**Where:** `crates/glyim-codegen/src/lib.rs:1060-1063` (and `generate`, :553-567)
All targets (`Goto`, `JumpIf`, `SwitchInt`, `Call` resume, `Assert`) are written as raw `BasicBlockIdx` values; `generate()` writes only concatenated bytes. `block_offsets` is populated nowhere in the repo except the VM's own test assembler. Any control-flow-bearing output jumps into the middle of an instruction. Additionally `OP_CALL`'s stack-pushed fn index is `intern_fn`'s first-encounter order, not the module function order.

**Fix (do RT-3/RT-4/RT-5 together with this as one "bytecode contract" PR):**
1. Have `generate_function` also return `(n_locals, arg_count, block_offsets)` — computable with the existing `decode_bytecode` helper: record each block's start offset when emitting (keep a `HashMap<BasicBlockIdx, u32>` filled in `emit_terminator`'s caller before each block's first byte).
2. Add a module serializer: `generate()` writes a header (magic, fn count), then per function: `n_locals: u32, arg_count: u32, offsets_len: u32, pairs…, code_len: u32, code…`. Add the matching `Module::deserialize` in the VM crate.
3. Map `intern_fn` ids to module function indices: emit a placeholder u32 per `OP_CALL`, patch all of them in a fixup list at function end once the module order is known.
4. End-to-end test: `generate()` → `Module::deserialize` → `Vm::run` for a program with if/else, a loop, and a call — outputs must match the interpreter.

<a name="rt-13"></a>
### [RT-13] [C] Array/slice indexing stride uses `size_of(container)` instead of element size
**Where:** `crates/glyim-codegen/src/lib.rs:291-296` (same bug :377 for `ConstantIndex`, :440-441 for `Subslice`)
At that point `current_ty` is the **array/slice being indexed** (`local_tys[place.local].ty`, :269); the element type is only assigned *after* the arm (:347-355). `a[i]` computes `base + i*16` for `[i32;4]` — reads/writes into neighboring locals. For a slice, `layout_of(Slice)` errors → 0 → every index aliases element 0.

**Fix:**
```diff
                 ProjectionElem::Index(local) => {
-                    let elem_size = self.layout_provider.size_of(current_ty);
+                    // current_ty is the CONTAINER; index by the ELEMENT size.
+                    let elem_ty = match self.ctx().map(|c| c.ty_kind(current_ty)) {
+                        Some(TyKind::Array(elem, _)) | Some(TyKind::Slice(elem)) => *elem,
+                        _ => current_ty,
+                    };
+                    let elem_size = self.layout_provider.size_of(elem_ty);
```
Apply the identical fix at the `ConstantIndex` arm (:377) and in the `Subslice` stride math (:440-441).

**Check:** bytecoded `a[1] = 7` on `[i32;4]`; dump memory — only slot 1 changed.

<a name="rt-14"></a>
### [RT-14] [H] Peephole constant folding of `OP_REM` panics the compiler on `% 0` (ICE at -O1)
**Where:** `crates/glyim-codegen/src/lib.rs:522-530`
The doc claims div-by-zero isn't foldable, but only `OP_DIV` is guarded; `a.rem(b)` panics on `b == 0` and `i64::MIN % -1`.

**Fix:**
```diff
-        OP_REM => a.rem(b),
+        OP_REM => a.checked_rem(b)?,
```
**Check:** `let x = 5 % 0;` at -O1 compiles (runtime trap per RT-7), no compiler panic.

<a name="rt-15"></a>
### [RT-15] [H] `tag_offset()` returns `tag_size`, but layout puts payload at `align_to(tag_size, payload_align)` — enum payload field accesses read wrong bytes
**Where:** `crates/glyim-codegen/src/lib.rs:140-160` vs `crates/glyim-layout/src/lib.rs:709, 717-724`
`enum E { A(u64), B }` (1-byte tag, 8-byte payload): `data_start == 8`, but `Downcast` adds `tag_offset() == 1` → every payload field read/written at offset 1 instead of 8 (misaligned, wrong values).

**Fix:** export the real payload start from layout and use it. In `crates/glyim-layout/src/lib.rs`, expose on the layout struct (it already computes it at :724):
```rust
pub struct Layout { …, pub variant_data_offset: Bytes, … }  // = tag_size.align_to(variant_data.align) when Direct
```
Then in codegen:
```diff
-                tag_size.0
+                layout.variant_data_offset.0
```
**Check:** bytecoded enum with `(u64)` payload reads the right value; add the same for the LLVM path via a run-pass test.

<a name="rt-16"></a>
### [RT-16] [P] `intern_string`/`intern_fn` are O(n) linear scans → O(n²) codegen interning
**Where:** `crates/glyim-codegen/src/lib.rs:451-471`

**Fix:** keep a lookup map beside the Vec:
```rust
pub struct Backend {
    string_table: RefCell<Vec<String>>,
    string_index: RefCell<HashMap<String, u32>>,   // add
    fn_table: RefCell<Vec<(FnDefId, Substitution)>>,
    fn_index: RefCell<HashMap<(FnDefId, u64 /*subst hash*/), u32>>,  // add
}
```
On insert, check the map first; append to both on miss. (For `Substitution`, key by a cheap hash — e.g. hash the interned Ty handles — or intern substitutions themselves.)

<a name="rt-17"></a>
### [RT-17] [P] Layout provider re-creates the layout computer and recomputes layouts with no memoization on every query
**Where:** `crates/glyim-codegen/src/lib.rs:117-129` (also :93-115, :140-160)
Every `field_offset`/`size_of`/`tag_offset` constructs a fresh `SimpleLayoutComputer` and walks the full recursive layout again — O(subtree) per projection, quadratic for deep struct chains.

**Fix:** add `layout_cache: RefCell<HashMap<Ty, Layout>>` on `GlyimLayoutProvider` keyed by the interned `Ty` handle; construct one `SimpleLayoutComputer` per provider (store it). Invalidate when the `TyCtxHandle`'s `Arc` pointer changes (store the last-seen `Arc` ptr and clear the map on change).

<a name="rt-18"></a>
### [RT-18] [M] CLI silently drops `--opt-level` for the bytecode backend and warns it "has no effect"
**Where:** `crates/glyim-cli/src/lib.rs:526-540`
`BytecodeBackend::with_ty_ctx_handle(...)` (:539) hard-codes `OptLevel::O0` although `with_opt_level` and the peephole pass exist (codegen :168-179, :580-582).

**Fix:**
```diff
     } else if args.backend == "bytecode" {
-        if args.opt_level > 0 {
-            tracing::warn!("bytecode backend opt-level currently has no effect; …");
-        }
+        // (peephole decoder must be fixed first — see RT-5)
```
and in the backend construction:
```diff
-    BytecodeBackend::with_ty_ctx_handle(handle)
+    BytecodeBackend::with_ty_ctx_handle(handle)
+        .with_opt_level(match args.opt_level { 0 => OptLevel::O0, 1 => OptLevel::O1, 2 => OptLevel::O2, _ => OptLevel::O3 })
```

<a name="rt-19"></a>
### [RT-19] [M] `--backend=bytecode --emit=exec` feeds raw bytecode to the native linker
**Where:** `crates/glyim-cli/src/lib.rs:525-549` (link step :573-600)

**Fix:** right after the bytecode branch is entered:
```diff
     } else if args.backend == "bytecode" {
+        if matches!(args.emit, EmitKind::Exec | EmitKind::Cdylib) {
+            return Err("the bytecode backend supports --emit=obj only; \
+                        run the output with the glyim-bytecode-vm".into());
+        }
         if args.opt_level > 0 { … }
```
**Check:** the flag combination produces the clear error.

<a name="rt-20"></a>
### [RT-20] [H] Niche encoding leaves variants *after* the niche-holding variant unrepresentable
**Where:** `crates/glyim-layout/src/lib.rs:530-535`
`enum E { A, B(bool), C }` — niche in variant 1 — encodes `0..=0`: variant `C` has no discriminant value at all; matches on `C` mis-dispatch.

**Fix:**
```diff
+        // Only niche-encode when the non-niche variants form a contiguous range:
+        let encodable = niche_variant_idx == 0 || niche_variant_idx == variant_count - 1;
+        if !encodable { return None; }   // fall through to direct tagging
         let niche_variants = if niche_variant_idx == 0 {
```
(Full fix: add an explicit variant→niche-value map to `TagEncoding::Niche`; the guard is the safe minimal fix.)

**Check:** `layout_of(E)` above uses Direct tagging; match dispatch correct.

<a name="rt-21"></a>
### [RT-21] [C] `glyim_thread_current_id` (pthread_self) is not in the same ID space as `glyim_thread_unpark` → **all async socket I/O hangs forever**
**Where:** `crates/glyim-runtime/src/lib.rs:1544-1563`; consumer `reactor.rs:343-345`; callers `glyim-lang-std/lib/net.g:270`, `thread.g:193-206`
`glyim_thread_unpark` looks up the `ThreadStore` map keyed by spawn-assigned ids (1, 2, 3…). The reactor's FFI contract is "wake executor `thread_id` via `glyim_thread_unpark`", but `.g` futures pass `thread::current_id()` → `pthread_self()` (huge address values, never a store key; the executor/main thread isn't even registered). `unpark` silently finds nothing — `ReadFuture`/`WriteFuture` never re-awaken.

**Fix:** make `current_id` return the store id. Add a thread-local current-id:
```rust
static CURRENT_THREAD_ID: std::cell::Cell<usize> = std::cell::Cell::new(0);
thread_local! { static CURRENT_ID: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_thread_current_id() -> usize {
    CURRENT_ID.with(|c| c.get())
}
```
- In `glyim_thread_spawn`'s entry closure (lib.rs:~1437-1441), after registering, call `CURRENT_ID.with(|c| c.set(id))` at thread start.
- At runtime init (pub fn `init()` or the first FFI call), register the main thread in `threads()` and set the main thread's `CURRENT_ID` to its id.

**Check:** `glyip test` on `tests/runtime/tcp.rs` or an async echo program — completes instead of hanging.

<a name="rt-22"></a>
### [RT-22] [H] `register_fd` takes ownership of a raw fd also owned by the socket table — double-close / fd-reuse hazard
**Where:** `crates/glyim-runtime/src/reactor.rs:299-308`
`TcpStream::from_raw_fd(fd)` transfers ownership; on deregister/shutdown its Drop closes the fd while `tcp_streams()` still holds its own `TcpStream` for the same descriptor — a later socket can receive the recycled number and get closed by the wrong owner.

**Fix:** don't take ownership — use `BorrowedFd`:
```diff
-                    let src = unsafe { mio::net::TcpStream::from_raw_fd(fd) };
-                    sources.lock().unwrap().insert(token, Box::new(src));
+                    // Non-owning registration: the runtime's TcpStream remains the
+                    // sole owner. mio's Registry can poll any RawFd via a custom
+                    // Source; here we store a guard that NEVER closes the fd.
+                    let src = unsafe { NonOwningSource::new(fd) };  // custom impl of mio::event::Source using BorrowedFd
+                    sources.lock().unwrap().insert(token, Box::new(src));
```
Implement `NonOwningSource` with `std::os::fd::BorrowedFd::borrow_raw(fd)` and `mio::net::TcpStream::from` unavailable — if mio requires owned, register via `poll.registry().register(&mut evented_fd, …)` using `socket2`'s borrowed view, or `libc::poll`-based registration. Document the invariant: "reactor registrations never own fds".

**Check:** run a server that closes a client socket then opens a new one — no misdirected close (strace or fd-count test).

<a name="rt-23"></a>
### [RT-23] [M] `register_fd` silently ignores a dead/inert reactor — futures park forever
**Where:** `crates/glyim-runtime/src/reactor.rs:179-190` (fallback :214-230)
`.send(Command::RegisterFd{..}).ok()` discards channel failure; the inert fallback reactor drops its receiver immediately.

**Fix:**
```diff
-        self.tx.send(Command::RegisterFd { token, fd, interest, thread_id }).ok();
-        Ok(token)
+        self.tx.send(Command::RegisterFd { token, fd, interest, thread_id })
+            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe,
+                "reactor is not running"))?;
+        Ok(token)
```
and have `glyim_reactor_register` return the `usize::MAX` error sentinel on failure so `.g` futures can surface a diagnostic instead of parking.

<a name="rt-24"></a>
### [RT-24] [H] `block_on`'s waker is unreachable from outside — the reactor can never wake a pending future
**Where:** `crates/glyim-runtime/src/async_runtime.rs:106-121` (with :70-76)
`Context::new()` allocates a private condvar pair; `Reactor::register(source, waker, interest)` needs a `Waker` no API can obtain. Any future that returns `Pending` without waking synchronously deadlocks.

**Fix:** expose the waker:
```diff
 pub fn block_on<F: Future>(mut future: F) -> F::Output {
-    let mut cx = Context::new();
+    let cx = Context::shared();           // returns Arc<WakerState>
+    REACTOR_GLOBAL.set_waker(cx.waker()); // reactor wakes THIS waker on fd events
     loop {
-        match future.poll(&mut cx) {
+        match future.poll(&mut cx.context()) {
```
- Add `Context::shared()` + `Context::context()` in async_runtime (:70-76), reusing the existing pair.
- In `reactor.rs`, replace the per-slot fresh `Waker::new()` (:175) with the shared waker registered above (store it in a `OnceLock<Arc<WakerState>>` on the reactor).

**Check:** async TCP echo test completes (combined with RT-21/RT-23).

<a name="rt-25"></a>
### [RT-25] [H] `glyim_process_wait`/`wait_output` hold the registry lock across a blocking wait — `kill` deadlocks, all spawns stall
**Where:** `crates/glyim-runtime/src/lib.rs:710-714` (same :767-770; kill needs the lock at :833-835)

**Fix:**
```diff
     let mut registry = process_registry().lock().expect("process registry lock poisoned");
     if let Some(mut child) = registry.children.remove(&handle) {
-        match child.wait() {
+        drop(registry);           // never block while holding the global lock
+        match child.wait() {
```
(`wait_output`: same `drop` before `wait_with_output`.)

**Check:** thread A waits on `sleep 30`, thread B kills it — B returns promptly.

<a name="rt-26"></a>
### [RT-26] [M] Spawned children always get piped stdout/stderr but plain `wait()` never drains — pipe-full deadlock
**Where:** `crates/glyim-runtime/src/lib.rs:671-672` (with :706-728)
A child writing >64KB before exit blocks forever; `child.wait()` doesn't read pipes.

**Fix:** drain in the wait path:
```diff
     if let Some(mut child) = registry.children.remove(&handle) {
         drop(registry);
+        // Drain piped streams on threads so the child can never block on a full pipe.
+        let out = child.stdout.take().map(spawn_drain);
+        let err = child.stderr.take().map(spawn_drain);
         match child.wait() {
```
with `fn spawn_drain(mut r: ChildStdout) -> JoinHandle<()> { std::thread::spawn(move || { let mut b=[0u8;8192]; while let Ok(n)=r.read(&mut b) { if n==0 {break;} } }) }`. (Or default to `Stdio::inherit()` unless `wait_output` is used.)

<a name="rt-27"></a>
### [RT-27] [P] Blocking network/file I/O while holding the global registry mutexes
**Where:** `crates/glyim-runtime/src/lib.rs:1088-1143` (accept/read/write), :1283-1396 (udp); `fs.rs:261-309`
One process-global `Mutex` guards all sockets/files; a blocked `accept` freezes every other thread's I/O.

**Fix (per site, 6 sites):** look up under the lock, clone the handle, drop the guard, then block:
```diff
     let mut listener_store = tcp_listeners().lock().unwrap();
-    let listener = match listener_store.listeners.get_mut(&fd) { Some(l) => l, None => return -1 };
-    let (stream, _) = match listener.accept() {
+    let listener = match listener_store.listeners.get(&fd) { Some(l) => l, None => return -1 };
+    let listener = listener.try_clone().expect("listener clone");
+    drop(listener_store);
+    let (stream, _) = match listener.accept() {
```
(`try_clone()` for files/sockets; store per-handle `Arc<Mutex<…>>` as the cleaner alternative.)

<a name="rt-28"></a>
### [RT-28] [M] `glyim_time_now_nanos` returns only sub-second nanos — non-monotonic, resets every second
**Where:** `crates/glyim-runtime/src/lib.rs:1601-1606` (same shape :1631-1636)

**Fix:**
```diff
 pub unsafe extern "C" fn glyim_time_now_nanos() -> u64 {
-    monotonic_base().elapsed().subsec_nanos() as u64
+    monotonic_base().elapsed().as_nanos() as u64
 }
```
(same for `glyim_time_system_nanos`; grep `.g` stdlib callers for downstream assumptions).

<a name="rt-29"></a>
### [RT-29] [M] `alloc_ffi_bytes` returns null for empty data — an existing-but-empty env var reads as "missing"
**Where:** `crates/glyim-runtime/src/lib.rs:78-81` (consumer :224-238; same for args/current_dir/wait_output)

**Fix:**
```diff
 pub(crate) fn alloc_ffi_bytes(data: &[u8]) -> *mut u8 {
-    if data.is_empty() {
-        return std::ptr::null_mut();
-    }
+    // Allocate at least 1 byte so null strictly means "allocation failure".
+    let cap = data.len().max(1);
+    let layout = std::alloc::Layout::from_size_align(cap, 1).unwrap();
+    unsafe {
+        let ptr = std::alloc::alloc(layout);
+        if !ptr.is_null() { std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len()); }
+        ptr
+    }
 }
```
(`*out_len == 0` now correctly encodes "exists, empty".)

<a name="rt-30"></a>
### [RT-30] [M] Thread/child registries only grow — spawn-and-forget leaks forever
**Where:** `crates/glyim-runtime/src/lib.rs:1437-1441` (threads), :674-689 (children)

**Fix:** detach by default: on spawn, keep only what's needed for unpark (`Arc<Thread>`); drop the `JoinHandle` (never `join`ed anyway). For children, reap opportunistically: in `process_registry()`, before insert, sweep `children.retain(|_, c| c.try_wait().map(|s| s.is_none()).unwrap_or(true))`.

<a name="rt-31"></a>
### [RT-31] [C] Proc-macro cdylib is built into a temp dir that is **deleted before it is dlopened** — `--proc-macro-deps` can never succeed
**Where:** `crates/glyim-cli/src/lib.rs:715-716` (dir dropped at return :749; used at :697)
`out_dir` is a local `TempDir`; its Drop deletes the directory when `compile_proc_macro_dep` returns. `load_cdylib(cdylib_path…)` (:697) then opens a deleted path. Additionally `cdylib_path.to_str().unwrap_or_default()` (:697) silently passes `""` for non-UTF-8 paths — `dlopen("")` loads the *current process*.

**Fix:**
```diff
-fn compile_proc_macro_dep(dep: &std::path::Path, host_triple: &str) -> Result<std::path::PathBuf, String> {
-    let out_dir = tempfile::tempdir().map_err(|e| format!("failed to make temp dir: {e}"))?;
+fn compile_proc_macro_dep(dep: &std::path::Path, host_triple: &str)
+    -> Result<(tempfile::TempDir, std::path::PathBuf), String>
+{
+    let out_dir = tempfile::tempdir().map_err(|e| format!("failed to make temp dir: {e}"))?;
     …
-    Ok(cdylib_path)
+    Ok((out_dir, cdylib_path))   // caller keeps the TempDir alive until after load
 }
```
Update the caller to bind `(let _keep_alive, let cdylib_path) = …;` with `_keep_alive` living past the `load_cdylib` call, and:
```diff
-        glyim_proc_macro::load_cdylib(cdylib_path.to_str().unwrap_or_default(), …)
+        let path_str = cdylib_path.to_str()
+            .ok_or_else(|| "proc-macro cdylib path is not valid UTF-8".to_string())?;
+        glyim_proc_macro::load_cdylib(path_str, …)
```
**Check:** `glyim foo.g --proc-macro-deps pm.rs --emit obj` loads the macro (currently always fails).

<a name="rt-32"></a>
### [RT-32] [H] Linker flags flattened to a space-joined string and re-split on whitespace — paths with spaces break
**Where:** `crates/glyim-cli/src/linker.rs:117-134` (split at :49-53, :93-97)
`/Users/me/My Projects/deps` → `-L/Users/me/My` + `Projects/x`.

**Fix:** stop round-tripping through a string:
```diff
-    Some(parts.join(" "))
+    Some(parts)   // change LinkArgs::to_args to return Vec<String>
```
and in `UnixLinker::link`/`MsvcLinker::link`:
```diff
         if let Some(flags) = link_flags {
-            for flag in flags.split_whitespace() {
-                cmd.arg(flag);
-            }
+            for flag in flags {
+                cmd.arg(flag);           // each element is one argument
+            }
         }
```
Keep `split_whitespace` ONLY for the free-form user `--link-flags` string (that one is genuinely user-tokenized).

<a name="rt-33"></a>
### [RT-33] [L] ICE backtrace written to a fixed, predictable `/tmp/glyim-ice.txt` — concurrent compiles overwrite each other
**Where:** `crates/glyim-cli/src/lib.rs:157-158`

**Fix:**
```diff
-            let path = std::env::temp_dir().join("glyim-ice.txt");
+            let path = std::env::temp_dir().join(format!(
+                "glyim-ice-{}-{}.txt", std::process::id(),
+                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
+                    .map(|d| d.as_millis()).unwrap_or(0)));
```

---

## 5. `glyip` / `glyim-lsp` / `glyim-pipeline` / `glyim-span` / `tools/glyim-pilot` (32 findings)

<a name="inf-1"></a>
### [INF-1] [H] Registry resolution silently falls back to `versions.first()` when no version matches the requirement
**Where:** `crates/glyip/src/dep.rs:624-630`
`select_best_version(...).or_else(|| entry.versions.first().cloned())` locks the *first listed* version whenever the requirement matches nothing — order-dependent, may be old or a prerelease. This defeats the deliberate `DependencyNotFound` invariant tested at `tests/dep_semver.rs::resolve_parseable_but_unmatched_req_is_not_found`.

**Fix:**
```diff
                 let version = if let Some(req) = version_req {
                     select_best_version(&entry.versions, Some(req))
-                        .or_else(|| entry.versions.first().cloned())
                         .ok_or_else(|| GlyipError::DependencyNotFound {
```
**Check:** the existing semver test extended to the registry path passes; `foo = "^9.0"` against an index with 1.x errors.

<a name="inf-2"></a>
### [INF-2] [H] Resolver can lock two versions of the same crate; the conflict check only inspects the first locked entry
**Where:** `crates/glyip/src/dep.rs:442-452, 556-572` (+ `lockfile.rs:145-148`)
The visited-key includes the requirement string, so the same crate with two different req strings resolves **twice** and both entries are locked. `check_version_conflicts` then finds one entry satisfying all reqs → no error, but the lockfile holds two `foo` entries and downstream builds are ambiguous. The key format is also ambiguous: crate `foo-1.0` no-req collides with crate `foo` req `1.0`.

**Fix:**
1. Collect all requirements per name **before** resolving: change `visit_stack` entries to accumulate into `HashMap<Name, Vec<VersionReq>>`.
2. After BFS, per name pick the single version satisfying all reqs (`select_best_version` over the union) or emit `DependencyConflict`.
3. Key `visited` on `(name, source)` and make the key unambiguous: `format!("{}@{}", name, source_tag)`.

**Check:** new test with two reqs (`^1.0`, `>=1.2`) locking exactly one version; a genuinely unsatisfiable pair errors.

<a name="inf-3"></a>
### [INF-3] [H] Nested path-dependency base is the raw declared path — resolves against CWD, loses ancestors
**Where:** `crates/glyip/src/dep.rs:480-488, 505-517`
(a) A relative `base` is joined **without anchoring to `project_dir`** (CWD-dependent). (b) `dep_base` propagates the parent's *declared* path instead of its *resolved absolute dir*, so level ≥ 2 of nested path deps loses all ancestor components. `read_from_dir` failing silently (INF-4) hides the misresolution.

**Fix:**
```diff
                 let abs_path = path.as_ref().map(|p| {
                     if p.is_absolute() {
                         p.clone()
+                    } else if let Some(ref b) = base {
+                        // anchor the whole chain at the project dir:
+                        project_dir.join(b).join(p)
                     } else if let Some(ref b) = base {
                         b.join(p)
                     } else {
                         project_dir.join(p)
                     }
                 });
```
(collapse to the anchored branch), and:
```diff
-                let dep_base = if dep_path.is_some() { path.clone() } else { None };
+                // pass the RESOLVED directory of the crate just processed:
+                let dep_base = abs_path.map(|p| p);   // absolute PathBuf
```
keeping `dep_base` an absolute `PathBuf` end-to-end.

**Check:** 3-level nested path dep test (`libs/app` → `../util` → `sub/deep`) locks all three regardless of CWD.

<a name="inf-4"></a>
### [INF-4] [H] Path/git dep `Glyip.toml` read errors silently swallowed into a default manifest
**Where:** `crates/glyip/src/dep.rs:684-696` (same :752-764)
A missing dir or malformed manifest becomes an empty manifest (version 0.1.0, no deps) — transitive deps silently vanish from the lockfile.

**Fix:**
```diff
-        let config = GlyipToml::read_from_dir(path).unwrap_or_else(|_| GlyipToml { …default… });
+        let config = GlyipToml::read_from_dir(path).map_err(|e| GlyipError::ManifestRead {
+            path: path.display().to_string(),
+            source: e.to_string(),
+        })?;
```
**Check:** a path dep with a broken manifest fails `glyip build` with the path in the message.

<a name="inf-5"></a>
### [INF-5] [M] Transitive deps are looked up in the *root project's* git-dependency table
**Where:** `crates/glyip/src/dep.rs:455-464`
A registry dep of some transitive crate whose name collides with a root git dep is rerouted through git with the root's URL.

**Fix:** carry the source through the queue — extend the work item with `source: DepSource` (enum `Root | Registry | Git(GitSpec) | Path(PathBuf)`) set by whoever enqueues it; the git-table lookup runs only for `DepSource::Root`.

<a name="inf-6"></a>
### [INF-6] [H] `validate_against_manifest` treats a semver *requirement* as an exact lockfile key — `1.0` ≠ `1.0.0` → false VersionMismatch
**Where:** `crates/glyip/src/lockfile.rs:181-207`
`dep.version()` returns the requirement string; `get_crate` does an exact `format!("{}-{}", name, version)` lookup. Every dep that doesn't pin full `X.Y.Z` false-positives.

**Fix:**
```diff
             match dep.version() {
                 Some(manifest_version) => {
-                    match self.get_crate(name, manifest_version) {
-                        Some(_) => {}
-                        None => { …mismatch error… }
+                    // Requirement vs exact version:
+                    let locked = self.crates().find(|c| c.name == *name);
+                    let ok = match (Version::parse(manifest_version), locked) {
+                        (Ok(exact), Some(c)) => c.version == *manifest_version,
+                        (_, Some(c)) => VersionReq::parse(manifest_version)
+                            .map(|req| req.matches(&Version::parse(&c.version).unwrap()))
+                            .unwrap_or(false),
+                        (_, None) => false,
+                    };
+                    if !ok { …mismatch error… }
```
**Check:** manifest `foo = "1.0"` + lockfile `foo-1.0.5` validates cleanly.

<a name="inf-7"></a>
### [INF-7] [H] Incremental fingerprint check misses *deleted* source files — stale binaries served
**Where:** `crates/glyip/src/fingerprint.rs:238-243` (in `has_any_changed`, :228-253)
Only currently-existing files are walked; a deleted file's stale fingerprint never triggers a rebuild. Stale `.fp` entries also accumulate forever (:178-191).

**Fix:**
```diff
         let files = collect_files_with_extension(dir, extension);
         for path in &files {
             if self.has_changed(path)? {
                 return Ok(true);
             }
         }
+        // A fingerprinted file that no longer exists counts as a change.
+        if self.fingerprints.keys().any(|p| !std::path::Path::new(p).exists()) {
+            return Ok(true);
+        }
```
and in `mark_built`/`save_to_dir` prune entries whose paths no longer exist:
```rust
self.fingerprints.retain(|p, _| std::path::Path::new(p).exists());
```
**Check:** build, delete a module file, build again — recompilation happens.

<a name="inf-8"></a>
### [INF-8] [M] Config fingerprinting tracks `glyim.toml`/`glyim.lock` — wrong filenames for this project
**Where:** `crates/glyip/src/fingerprint.rs:294-315`
Real names are `Glyip.toml` (config.rs:8) and `Glyip.lock` (lockfile.rs:11). Lockfile edits never invalidate the cache.

**Fix:**
```diff
     let mut files = vec![
-        dir.join("glyim.toml"),
-        dir.join("glyim.lock"),
+        dir.join("Glyip.toml"),
+        dir.join("Glyip.lock"),
         dir.join("build.g"),
         dir.join("build.rs"),
     ];
```
and fingerprint `Glyip.lock` in `Cache::mark_built` (cache.rs:115-119).

<a name="inf-9"></a>
### [INF-9] [H] Compiled-test temp object/executable name collides across projects and processes
**Where:** `crates/glyip/src/commands.rs:417-423`
Name derived only from the file base name. Two parallel `glyip test` runs (or `src/main.g` + `tests/main.g`) race: A's object overwritten by B before A links → wrong pass/fail.

**Fix:**
```diff
     let output_path = std::env::temp_dir().join(format!(
-        "glyim_test_compiled_{}.o",
-        file.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
+        "glyim_test_compiled_{}_{}.o",
+        std::process::id(),
+        // hash the full path so the same-named files don't collide either:
+        { use std::hash::{Hash, Hasher}; let mut h = std::collections::hash_map::DefaultHasher::new();
+          file.hash(&mut h); h.finish() }
     ));
```
and delete both artifacts after the run (`let _ = std::fs::remove_file(&output_path); let _ = std::fs::remove_file(&exe_path);`).

<a name="inf-10"></a>
### [INF-10] [P] `cmd_test` deep-clones the entire body set for every test function
**Where:** `crates/glyip/src/commands.rs:338-341`
O(B×T) clones of full MIR bodies per test run.

**Fix:** group tests by file; build ONE `Interpreter` per file:
```diff
-        for (id, b) in &compilation.bodies {
-            interpreter.add_function(*id, (**b).clone());
-        }
+        // one interpreter per file, reused across its tests:
+        let mut interp_for_file = |interp: &mut Interpreter| {
+            for (id, b) in &compilation.bodies { interp.add_function(*id, (**b).clone()); }
+        };
```
or (better) change `Interpreter::add_function` to take `Rc<Body>`/`Arc<Body>` and clone the `Arc` (pairs with MIR-19).

---

### `glyim-lsp` (12 findings)

<a name="inf-11"></a>
### [INF-11] [C] `SourceMap` treats LSP UTF-16 columns as byte offsets (both directions)
**Where:** `crates/glyim-lsp/src/database.rs:64-74` (`line_col_to_offset`) and :46-62 (`span_to_position`)
LSP positions are UTF-16 code units; this adds them to byte line starts. On any line with non-ASCII text every hover/goto-def/rename/completion after that char is wrong; no clamp to line end either. Server-emitted positions drift the same way.

**Fix:**
```rust
// Precompute per-line char info once in SourceMap::new:
struct LineInfo { start_byte: usize, // plus lazily computed mapping
}
pub fn line_col_to_offset(&self, line: usize, col16: usize) -> Option<usize> {
    let start = *self.line_starts.get(line)?;
    let line_text = self.content.get(start..).unwrap_or("");
    let mut u16_count = 0usize;
    for (i, ch) in line_text.char_indices() {
        if u16_count >= col16 { return Some(start + i); }
        u16_count += ch.len_utf16();
    }
    if u16_count == col16 {
        // end of line (before the trailing newline)
        Some(start + line_text.lines().next().map_or(0, |l| l.len()))
    } else {
        None // col beyond end of line — do NOT spill into the next line
    }
}
pub fn span_to_position(&self, lo: usize, hi: usize) -> … {
    // walk self.content[..lo].chars() counting len_utf16 for `character`
}
```
**Check:** round-trip test `offset_to_position(line_col_to_offset(l, c)) == (l, c)` on fixtures containing `é`, emoji, and CJK.

<a name="inf-12"></a>
### [INF-12] [C] Identifier extraction mixes byte offsets with `Vec<char>` indices — panic + wrong symbols on non-ASCII sources
**Where:** `crates/glyim-lsp/src/navigation.rs:22-35` (same in rename.rs:75-88, goto_definition.rs:23-35)
`offset` is bytes but indexes `chars`; `source[start..end]` slices a String with char indices → panic "not a char boundary" whenever the walk crosses a multibyte char. `is_alphabetic` also excludes digits (`foo1` extracts as `foo`).

**Fix (pure byte-offset walk):**
```rust
fn identifier_at(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if offset > source.len() || !source.is_char_boundary(offset) { return None; }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start = source[..offset].char_indices().rev()
        .take_while(|(_, c)| is_word(*c))
        .map(|(i, c)| i).last().unwrap_or(offset);
    let end = source[offset..].char_indices()
        .take_while(|(_, c)| is_word(*c))
        .map(|(i, c)| offset + i + c.len_utf8()).last().unwrap_or(offset);
    if start == end { None } else { Some(source[start..end].to_string()) }
}
```
**Check:** unit tests with `αβγ ident`, `foo1`, `x é y` — no panic, correct symbols.

<a name="inf-13"></a>
### [INF-13] [C] Rename produces whole-statement/expression TextEdits — **renaming clobbers surrounding code**
**Where:** `crates/glyim-lsp/src/reference_graph.rs:479-488` (Let), :502-516 (Assign), :231-234 (Call), :274-281 (MethodCall), :306-313 (FieldAccess); consumed by rename.rs:110-129
The recorded `span` is the whole enclosing expression (`node_span(node)` at lower_expr.rs:1798). Renaming `x` in `let x = 5;` **deletes the statement** and writes `z` in its place. Same for calls (`f(a,b)` replaced whole), methods, fields. Existing rename tests assert edit counts and `new_text` only, never ranges.

**Fix:** record the *identifier's own* span. At each `add_ref` site, use the sub-expression's span:
```diff
                 Expr::Assign { lhs, rhs } => {
                     if let Expr::Path(path) = &body.exprs[*lhs]
                         && let Some(name) = path.as_name()
                     {
+                        let ident_span = body.exprs[*lhs].span(); // the Path node's span, not the Assign's
                         add_ref(
                             &name_str,
-                            span,
+                            ident_span,
                             true, …
```
Do the same for `Expr::Let` (pattern binding's name span), Call (func path span), MethodCall (method-name segment span), FieldAccess (field-name span). If HIR doesn't store sub-spans yet, compute them from the syntax node during lowering (the CST node for each expr is available at `lower_expr` — store `body.expr_spans[id] = node_span(sub_node)` for the identifier node specifically, or re-lex the identifier text range at reference time).

Then add the missing test:
```rust
#[test] fn rename_applies_cleanly() {
    let edits = …rename…;
    let mut text = source.clone();
    // apply edits right-to-left; assert final text == expected
}
```
**Check:** rename `x`→`z` in `let x = 5; use(x);` yields `let z = 5; use(z);` — nothing else changed.

<a name="inf-14"></a>
### [INF-14] [H] Rename is scope-unaware — renaming a local renames same-named symbols everywhere
**Where:** `crates/glyim-lsp/src/rename.rs:98-138` (+ reference_graph.rs:773-778)
The reference graph is keyed by identifier text only.

**Fix (interim, shippable):** restrict rename to references inside the same file and same top-level item:
```diff
     let references = ref_graph.find_references(symbol_name);
-    let references: Vec<_> = references.into_iter()
-        .filter(|r| r.file_id == file_id)          // same file at minimum
+    let references: Vec<_> = references.into_iter()
+        .filter(|r| r.file_id == file_id && r.enclosing_item == cursor_item)
         .collect();
```
(store `enclosing_item` (item span or id) on each `Reference` while walking bodies — the walk already knows the body). Longer term: attach resolved `DefId`s.

<a name="inf-15"></a>
### [INF-15] [P] `walk_expr` invoked once per expression node as a root — quadratic re-walk per rebuild
**Where:** `crates/glyim-lsp/src/reference_graph.rs:753-768` (with recursive walk at :209-751)

**Fix:**
```diff
         for (_, body) in hir.bodies.iter_enumerated() {
             for param in &body.params { walk_pattern(…); }
             for (expr_id, _) in body.exprs.iter_enumerated() {
-                walk_expr(expr_id, body, …);
+                // Only walk ROOT expressions; children are visited recursively.
+                if is_root_expr(body, expr_id) {   // no other expr lists it as child
+                    walk_expr(expr_id, body, …);
+                }
```
Implement `is_root_expr` by precomputing a `HashSet<ExprId>` of all child ids in one pass. **Check:** hover latency on a 2k-line file (manual or benchmark) drops.

<a name="inf-16"></a>
### [INF-16] [C] Production LSP routes requests against a never-populated `FileMap`; no didOpen/didChange at all
**Where:** `crates/glyim-lsp/src/handler.rs:29, 76-77` (with server.rs:9-20)
`build_router` creates a fresh local `FileMap` nothing writes to; every request handler returns `None` → **all features permanently return null** in `run_server`. No DidOpen/DidChange/DidClose notifications are registered, so the driver never receives anything. `state.rs` implements document sync but is only used by tests. The two halves were never wired.

**Fix:**
```diff
-    let file_map = Arc::new(parking_lot::RwLock::new(FileMap::new()));
+    // Share the analysis driver's map — do not create a second one.
+    let file_map = db.file_map.clone();
```
and register the notifications:
```rust
router.notification::<DidOpenTextDocumentParams>("textDocument/didOpen", |params, state| {
    let _ = state.driver_tx.try_send(AnalysisMessage::FileOpened { …from params… });
    Ok(())
});
// same for didChange (FULL sync — send full content), didClose
```
where `state` carries `driver_tx` (the existing `LspState` channels). Formatting/folding already read `db.file_map` — they become consistent automatically.

**Check:** start the server, open a file in an LSP client — hover/completion/diagnostics all respond with real data.

<a name="inf-17"></a>
### [INF-17] [H] Analysis updates silently dropped when the bounded channel is full → stale index
**Where:** `crates/glyim-lsp/src/state.rs:65-71` (and :49-55)
`try_send` on a 16-slot channel while the driver does a full rebuild per message; failures are discarded (`let _ =`).

**Fix:** coalesce latest-wins per path:
```rust
// state.rs: replace driver_tx with Arc<Mutex<HashMap<PathBuf, AnalysisMessage>>> pending
// did_change: pending.lock().insert(path, msg);  // latest wins, never lost
// driver loop: drain the map (take()), process each path once.
```
(Or use `tokio::sync::mpsc::unbounded_channel` + drain-to-latest in the driver.)

<a name="inf-18"></a>
### [INF-18] [H] Closing a file leaves every derived cache stale; reopen mismatches FileIds; dependents never re-analyzed
**Where:** `crates/glyim-lsp/src/driver.rs:74-77, 148-155`
(a) `FileClosed` removes only `file_map` + dep-graph entries — closed files' symbols keep appearing in workspace symbols/completions. (b) Reopen reuses the old VFS FileId while `FileMap::get_or_create` allocates a new one — id spaces diverge, diagnostics read the wrong entry. (c) `extract_dependencies` is an empty placeholder and `affected_files` is never consulted.

**Fix:**
1. On close: `self.source_maps.remove(&id); self.symbol_index.remove_file(id); self.reference_graph… ; self.hirs…; self.diagnostics…;` (mirror every map keyed by file id).
2. Single FileId source: pass the VFS id into `FileMap::get_or_create` (or key the AnalysisDatabase by path everywhere).
3. Implement `extract_dependencies` minimally: after building the def-map, for every `mod foo;` in the file add edge `path → resolved_mod_file`; in `analyze_file`, after analyzing, enqueue `dep_graph.affected_files(path)` minus already-analyzed.

**Check:** open, close, reopen a file — diagnostics for it still correct; edit a `mod util;` target — importers re-analyze.

<a name="inf-19"></a>
### [INF-19] [H] Completion slices source at a possibly mid-character offset — panic on non-ASCII
**Where:** `crates/glyim-lsp/src/completion.rs:108-112, 236-244`
`src[..offset]` panics when `offset` lands inside a multibyte char (a consequence of INF-11 + raw byte walks).

**Fix:**
```diff
     let src = sm.source();
-    if offset == 0 || !src[..offset].ends_with('.') {
+    if offset == 0 || !src.is_char_boundary(offset) || !src[..offset].ends_with('.') {
         return None;
     }
```
and in `typed_identifier_prefix`, walk with `char_indices` (see INF-12's helper) so `start` is always a boundary; early-return `None` when `!src.is_char_boundary(offset)`.

<a name="inf-20"></a>
### [INF-20] [P] Per-keystroke full HIR scans in completion; `lookup_by_location` linear-scans despite an index existing
**Where:** `crates/glyim-lsp/src/database.rs:192-247`; `symbol_index.rs:350-360`
`type_at_offset` traverses all exprs of all bodies (twice) per request; `lookup_by_location` linearly scans all file symbols even though `by_location` is built (:337) but unused; `touch`/`evict_stale` are empty stubs.

**Fix:** at analysis time (not request time), build per-file sorted `Vec<(lo, hi, ExprId)>` for method/field receivers and a sorted symbol interval list; binary-search by offset in both lookup paths. Implement `evict_stale` with the existing `file_access_times` (LRU by timestamp, cap e.g. 64 files).

<a name="inf-21"></a>
### [INF-21] [M] External diagnostics always attributed to `FileId::from_raw(0)`
**Where:** `crates/glyim-lsp/src/diagnostics.rs:107-111`
The `file_name` is parsed then explicitly discarded.

**Fix:**
```diff
                 let file_id = span
                     .get("file_name")
                     .and_then(|f| f.as_str())
-                    .map(|_name| FileId::from_raw(0))
-                    .unwrap_or(FileId::from_raw(0));
+                    .and_then(|name| db.file_map.read().get_by_path(Path::new(name)))
+                    .map(|(id, _)| id)
+                    .unwrap_or(FileId::from_raw(u32::MAX)); // "unknown file" sentinel
```
**Check:** diagnostics from file B no longer appear on file A.

<a name="inf-22"></a>
### [INF-22] [H] Folding/formatting count braces and commas **inside string literals and comments** — corrupted edits
**Where:** `crates/glyim-lsp/src/folding.rs:10-27`; `formatting.rs:10-54`
`let s = "}";` pops the enclosing block's brace (every later range shifted); `format_code` reformats inside strings (`"a{b}"` becomes multiline; `"hello, world"` gains a doubled space).

**Fix:** run the real lexer and skip trivia/literals:
```rust
use glyim_frontend::lex;
let tokens = lex(source, file_id);   // same call rename_text_fallback already makes
// folding: iterate tokens; push/pop brace ranges on Punct('{')/Punct('}') only,
// skipping StringLit/CharLit/Comment kinds.
// formatting: only insert whitespace between token pairs where the grammar allows,
// never inside a literal/comment token's text.
```
Also fix `format_document`'s full-range end (`lines().count()` is one past the last valid line — use `saturating_sub(1)` + last line length).

---

### `glyim-pipeline` / `glyim-span` (3 findings)

<a name="inf-23"></a>
### [INF-23] [C] Slice drop glue loop **exits immediately, drops nothing**
**Where:** `crates/glyim-pipeline/src/mono_cache.rs:837-863`
The generated loop header assigns `idx = 0` and switches on it with branch `(0, exit_bb)` — "exit when idx == 0". The very first evaluation takes `exit_bb`: **zero elements are dropped** (leaked memory / missed destructors for `[T]` where `T: Drop`, e.g. `[String]`). Had the condition been inverted, the increment block's `Goto { block 0 }` (:803) would re-run the header which *resets idx to 0* — non-terminating either way. `len_local` is never used in any comparison.

**Fix:** emit a real comparison and split header/latch:
1. Allocate a `cond_local: LocalIdx` (bool) in the body's local decls.
2. Header block: keep `idx = 0` assignment ONLY on entry (move it to a separate `init` block before the header).
3. Header terminator:
```rust
block_header.statements.push(Statement {
    kind: StatementKind::Assign(Place::new(cond_local),
        Rvalue::BinaryOp(BinOp::Lt, Box::new((
            Operand::Copy(Place::new(idx_local)),
            Operand::Copy(Place::new(len_local)))))),
    source_info: SourceInfo::new(Span::DUMMY),
});
block_header.terminator = Terminator {
    kind: TerminatorKind::SwitchInt {
        discr: Operand::Copy(Place::new(cond_local)),
        switch_ty: Ty::BOOL,
        targets: SwitchTargets::new(Box::new([(1, body_bb)]), exit_bb),  // true → body
    }, … };
```
4. Latch (increment) block: `idx = idx + 1` then `Goto { header }` — and make the body's tail `Goto { latch }`, **not** the header.

**Check:** MIR-interp a `[String; 3]` slice drop with a drop counter — count == 3 (currently 0).

<a name="inf-24"></a>
### [INF-24] [H] Array drop glue indexes elements by *local variable* `#i` instead of constant index
**Where:** `crates/glyim-pipeline/src/mono_cache.rs:767-775` (used by `generate_array_drop_glue`, :738-762)
`ProjectionElem::Index(LocalIdx)` means "index by the runtime value stored in local `LocalIdx`" — the glue body has only a couple of locals, so for N > 1 it reads uninitialized locals as indices (out-of-bounds / wrong element dropped).

**Fix:**
```diff
 fn element_place_at(base: &Place, index: u32) -> Place {
-    let idx_local = LocalIdx::from_raw(index);
     let mut proj = base.projection.to_vec();
-    proj.push(ProjectionElem::Index(idx_local));
+    proj.push(ProjectionElem::ConstantIndex {
+        offset: index,
+        from_end: false,
+        min_len: index + 1,   // or false, per the variant's definition in glyim-mir
+    });
     Place { local: base.local, projection: proj.into_boxed_slice() }
 }
```
**Check:** interp `[String; 3]` array drop glue — 3 drops, elements 0..2.

<a name="inf-25"></a>
### [INF-25] [P] LSP driver does a full lex→typeck rebuild per keystroke with a fresh interner, no debounce, no hash gate
**Where:** `crates/glyim-lsp/src/driver.rs:90-105`

**Fix:** at the top of the per-message handler:
```rust
let hash = fxhash/fnv of content (or DefaultHasher);
if self.last_hash.get(&path) == Some(&hash) { return; }   // unchanged: skip entirely
self.last_hash.insert(path.clone(), hash);
```
plus coalescing from INF-17, and reuse a per-session `Interner` (names are interned strings — safe to share across analyses; clear per-file tables only).

<a name="inf-26"></a>
### [INF-26] [M] `HygieneCtx::adjust` can spin forever when a mark's context data is missing
**Where:** `crates/glyim-span/src/hygiene.rs:144-151` (with `remove_mark`, :111-128)
`remove_mark`'s OOB fallback returns the **same span unchanged**; `adjust`'s loop then never updates `current` → infinite loop. Latent (no production caller yet).

**Fix:**
```diff
     pub fn adjust(&mut self, span: Span, scope_ctx: SyntaxContext) -> Span {
         let mut current = span;
         while current.ctx != scope_ctx && !current.ctx.is_root() {
-            let (next, _) = self.remove_mark(current);
-            current = next;
+            match self.remove_mark_checked(current) {
+                Some((next, _)) => current = next,
+                None => break,  // or panic with an internal-error diagnostic
+            }
         }
         current
     }
```
where `remove_mark_checked` returns `None` when `span.ctx.to_raw()-1` is not a valid index (add it; keep the old fn for compat).

---

### `tools/glyim-pilot` (4 findings)

<a name="inf-27"></a>
### [INF-27] [H] Timed-out child processes are never killed — orphans accumulate
**Where:** `tools/glyim-pilot/src/process.rs:60-65`
tokio's `Command` defaults to `kill_on_drop(false)`; the timeout drops the `output()` future but the child keeps running forever.

**Fix:**
```diff
     let output_fut = tokio::process::Command::new(program)
         .args(args)
         .current_dir(cwd)
+        .kill_on_drop(true)
         .output();
```
**Check:** a command that ignores SIGTERM times out and the process is gone (`ps` shows nothing).

<a name="inf-28"></a>
### [INF-28] [H] Worktree path built from unvalidated `session_id` — path traversal out of `worktree_base`
**Where:** `tools/glyim-pilot/src/git_ops/worktree.rs:14-22`
`stream_id` comes from extension messages; `../../` escapes the base and materializes a checkout anywhere.

**Fix:**
```diff
+    if !stream_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
+        || stream_id.is_empty() || stream_id.len() > 64
+    {
+        return Err(format!("invalid stream id: {stream_id:?}"));
+    }
     let worktree_dir = worktree_base.join(format!("stream-{stream_id}"));
```
(validate `branch_version` the same way; it feeds a branch name too).

<a name="inf-29"></a>
### [INF-29] [H] `validate_path` never canonicalizes the accepted path — committed symlinks allow writes outside the worktree
**Where:** `tools/glyim-pilot/src/applier/security.rs:23-48`
Lexical cleaning only. A repo-committed symlink (`data -> /etc`) makes `fs::write` follow it outside the root (the canonicalize fallback only runs in the *escape* branch).

**Fix:**
```diff
     if !normalized.starts_with(&canonical_root) {
         …reject…
     }
+    // Symlink defense: canonicalize the deepest EXISTING ancestor and re-check.
+    let mut ancestor = normalized.clone();
+    while !ancestor.exists() { ancestor = ancestor.parent().map(|p| p.to_path_buf())
+        .ok_or_else(|| "no existing ancestor".to_string())?; }
+    let real = ancestor.canonicalize().map_err(|e| e.to_string())?;
+    if !real.starts_with(&canonical_root) {
+        return Err(format!("path escapes worktree via symlink: {relative_path}"));
+    }
```
**Check:** add a repo fixture with a symlink; the write attempt is rejected.

<a name="inf-30"></a>
### [INF-30] [M] Pilot panics on corrupt state file / port bind; `.glyim-tmp` name collides for concurrent applies
**Where:** `tools/glyim-pilot/src/main.rs:101-105, 123-124`; `applier/mod.rs:275`

**Fix:**
```diff
-    let persistence = Arc::new(StatePersistence::load(&project_root).await
-        .expect("failed to load state"));
+    let persistence = Arc::new(match StatePersistence::load(&project_root).await {
+        Ok(p) => p,
+        Err(e) => { tracing::error!("state load failed ({e}); starting fresh"); StatePersistence::new() }
+    });
```
```diff
-        let listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
+        let listener = tokio::net::TcpListener::bind(http_addr).await
+            .map_err(|e| format!("aux http bind {http_addr}: {e}"))?;
```
```diff
-    let tmp_path = abs_path.with_extension("glyim-tmp");
+    let tmp_path = abs_path.with_extension(format!("glyim-tmp-{}", std::process::id()));
```
(port configurable via a CLI/env var is the better fix for the bind).

<a name="inf-31"></a>
### [INF-31] [M] Whole-request results dropped via `?` inside per-item loops
**Where:** `crates/glyim-lsp/src/navigation.rs:93-98, 197-206`; `rename.rs:109`
One stale `file_id` aborts the entire find-references/rename response to `None`.

**Fix:**
```diff
     for r in references {
-        let sm = source_maps.get(&r.file_id)?;
+        let sm = match source_maps.get(&r.file_id) { Some(sm) => sm, None => continue };
         …
-        let path = file_map.path(r.file_id)?;
+        let path = match file_map.path(r.file_id) { Some(p) => p, None => continue };
```
(keep the outer `Option` only for the initial file resolution).

<a name="inf-32"></a>
### [INF-32] [L] Impl methods and pattern bindings get meaningless definition spans
**Where:** `crates/glyim-lsp/src/symbol_index.rs:140-146, 298-301`
Every method shares the impl block's span (goto-def collides, last wins); pattern locals get `Span::DUMMY`.

**Fix (interim):** use the method fn's own node span (available in the CST during indexing — pass the `FnDef` node down), and exclude `Span::DUMMY` symbols from the `by_location` index:
```diff
-                self.by_location.entry(span.lo).or_default().push(sym.clone());
+                if sym.definition.span != Span::DUMMY {
+                    self.by_location.entry(span.lo).or_default().push(sym.clone());
+                }
```
(Long term: thread pattern spans through HIR.)

---

## 6. `glyim-hir` / `glyim-meta` / `glyim-def-map` / `glyim-const-eval` / `glyim-lower` / `glyim-proc-macro` (35 findings)

<a name="hir-1"></a>
### [HIR-1] [C] Empty repetition `$()*` hangs the compiler in an infinite loop
**Where:** `crates/glyim-meta/src/expander/matcher.rs:483-505`
For a repetition with an empty body, `inner.is_empty()` is true so the zero-progress guard never fires; `match_pieces(&[], …)` returns `Ok((i, 0))` forever; `repetitions` grows unboundedly (hang → OOM).

**Fix:**
```diff
                     match match_pieces(inner, input, i, &mut rep_bindings) {
                         Ok((new_i, _matched_count)) => {
-                            if new_i == i && !inner.is_empty() {
+                            if new_i == i {
                                 break;
                             }
```
**Check:** expand `macro_rules! m { ($()* ) => { ok } }` — terminates (0 iterations).

<a name="hir-2"></a>
### [HIR-2] [C] Repetition substitution counts captured *tokens*, not matched iterations — multi-token fragments split
**Where:** `crates/glyim-meta/src/expander/substitution.rs:68-74` (matcher flattens at matcher.rs:509-534)
`m!(a + b, c)` for `($($e:expr),*)` binds `e = [a, +, b, c]` (4 trees, 2 iterations) → `repetitions = 4` → `$(let _ = $e;)*` expands **four** statements: `let _ = a; let _ = +; …` — silently wrong output.

**Fix:** make bindings depth-aware:
1. In matcher.rs:509-534, stop flattening: `bindings: HashMap<SmolStr, Vec<Vec<TokenTree>>>` — one inner Vec per matched iteration (the loop at :493 already has the grouping).
2. In substitution.rs:68-74:
```diff
-                        let repetitions: usize = var_names.iter()
-                            .filter_map(|name| bindings.get(name).map(|v| v.len()))
-                            .max().unwrap_or(0);
+                        // bindings are Vec<Vec<TokenTree>> — outer len = iteration count
+                        let repetitions: usize = var_names.iter()
+                            .filter_map(|name| bindings.get(name).map(|v| v.len()))
+                            .max().unwrap_or(0);
```
3. Where a metavar is emitted inside a repetition body, take the *whole* per-iteration slice for fragment-valued captures.

**Check:** `pair!(1+2, 3)` style macros expand to exactly 2 iterations.

<a name="hir-3"></a>
### [HIR-3] [C] Nested repetitions in the expansion template expand to nothing
**Where:** `crates/glyim-meta/src/expander/substitution.rs:128-141`
`find_all_metavars` only detects `$name` where `$` is immediately followed by an Ident **at the same level**. In `$( $( $x ),* ),*` the outer body is `[Dollar, Group(…), Comma, Star]` → `var_names` empty → 0 iterations. Also true for `$x` inside any delimiter group, e.g. `$(foo($x)),*`.

**Fix:**
```diff
 fn find_all_metavars(trees: &[TokenTree]) -> Vec<SmolStr> {
     let mut names = Vec::new();
-    let mut i = 0;
-    while i < trees.len() { …flat detection… }
+    fn walk(trees: &[TokenTree], names: &mut Vec<SmolStr>) {
+        let mut i = 0;
+        while i < trees.len() {
+            if let TokenTree::Token(SyntaxKind::Dollar, _) = &trees[i]
+                && i + 1 < trees.len()
+            {
+                match &trees[i + 1] {
+                    TokenTree::Token(SyntaxKind::Ident, name) => { names.push(name.clone()); i += 2; continue; }
+                    TokenTree::Group(g) => { walk(&g.inner, names); i += 2; continue; }
+                    _ => {}
+                }
+            }
+            if let TokenTree::Group(g) = &trees[i] { walk(&g.inner, names); }
+            i += 1;
+        }
+    }
+    walk(trees, &mut names);
     names
 }
```
(Pairs with HIR-2's depth-aware bindings — nested repetition depth indexing is part of that restructure.)

<a name="hir-4"></a>
### [HIR-4] [C] Macro-call arguments are captured as one `Group` — multi-metavar patterns like `($a:expr, $b:expr)` can never match
**Where:** `crates/glyim-meta/src/expander/mod.rs:486` (with token_tree.rs:82-84, matcher.rs:363-388)
`flatten_token_tree` returns `[Group(LParen,[1,Comma,2],RParen)]` — one element. Matching `($a:expr, $b:expr)` finds no top-level comma → `$a` greedily eats the whole group → pattern's `Comma` has no input → `NoMatch` → "no matching macro arm". Conversely `($x:expr)` matches the *entire* argument list (`maybe_add!(1, 2)` matches with `a = "(1, 2)"`).

**Fix:** unwrap the argument group before matching:
```diff
         let args = flatten_token_tree(args_node);
+        // Call-site arguments arrive as a single delimiter Group; match against
+        // their inner trees (same convention the arm pattern itself uses).
+        let args: Vec<TokenTree> = match args.as_slice() {
+            [TokenTree::Group(g)] if g.kind == GroupKind::Paren => g.inner.clone(),
+            _ => args,
+        };
         for arm in &def.arms {
```
**Check:** `macro_rules! pair { ($a:expr, $b:expr) => { ($a, $b) } }` + `pair!(1, 2)` expands; the existing `multiple_metavars` test passes for the right reason.

<a name="hir-5"></a>
### [HIR-5] [H] Macro hygiene mark is computed but never applied — `_mark` ignored
**Where:** `crates/glyim-meta/src/expander/mod.rs:1056-1098`
`ExpnData` pushed, `Mark` built, then passed as `_mark` and dropped — every expanded token gets `SyntaxContext::ROOT`; macro-introduced identifiers can capture/be captured by user identifiers.

**Fix (minimal, honest):** carry the mark through reparse and stamp spans:
1. Change `build_token_tree_green(tree, builder, _mark)` to record `(token_index, mark)` pairs into a side table on the expander.
2. Where expanded nodes get their spans assigned (the span-attachment pass consuming the reparse), look up each token's mark and set `SyntaxContext::apply_mark(mark)` instead of `ROOT`.
3. Add `debug_assert!` that the mark was consumed at least once per expansion so this can't silently regress.

**Check:** macro that binds `x` used where caller also has `x` — no capture collision.

<a name="hir-6"></a>
### [HIR-6] [M] Macro patterns containing a nested group silently drop the whole arm
**Where:** `crates/glyim-meta/src/expander/matcher.rs:156`
`TokenTree::kind()` returns `None` for `Group`; `tree.kind()?` propagates → `parse_pattern` returns None → `parse_macro_arm` silently drops the arm → misleading "no matching macro arm" at the call site.

**Fix:** either add `PatternPiece::Group { inner, open, close }` matching Group inputs structurally, or at minimum fail loudly:
```diff
         } else {
-            pieces.push(Piece::Token(tree.kind()?, tree.text()));
+            let kind = tree.kind().ok_or_else(|| MacroDefError::UnsupportedPattern)?;
             i += 1;
         }
```
and report "unsupported macro pattern element" with the arm's span instead of dropping.

<a name="hir-7"></a>
### [HIR-7] [M] Panics on short `StringLit` tokens in `first_string_lit`/`concat!`
**Where:** `crates/glyim-meta/src/expander/mod.rs:1212-1216` (and :823)
`text.as_str()[1..text.len() - 1]` underflows for a malformed 1-byte token (lexer error recovery can emit `"` alone) → slice panic (ICE).

**Fix:**
```diff
             TokenTree::Token(SyntaxKind::StringLit, text) => {
-                return Some(&text.as_str()[1..text.len() - 1]);
+                let s = text.as_str();
+                if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
+                    return Some(&s[1..s.len() - 1]);
+                }
+                return Some(s);
             }
```
(same guard at :823 in `concat!`; note :898's `concat_idents!` already uses the safe pattern).

<a name="hir-8"></a>
### [HIR-8] [M] Expansion reparse silently drops non-`FnDef` top-level nodes; `macro_rules!` inside expansions is dropped
**Where:** `crates/glyim-meta/src/expander/mod.rs:266-347, 352-354`

**Fix:**
```diff
                 for child in reparsed_root.children_with_tokens() {
                     match child {
-                        NodeOrToken::Node(n) => {
-                            if n.kind() == SyntaxKind::FnDef && … { … }
+                        NodeOrToken::Node(n) => {
+                            match n.kind() {
+                                SyntaxKind::FnDef => { …existing… }
+                                SyntaxKind::MacroDef => {
+                                    // register macros defined by expansions:
+                                    self.register_macro_def(&n)?;   // same path as collect_macros
+                                }
+                                other => diagnostics.push(internal_warn(
+                                    format!("macro expansion produced unexpected top-level node {other:?}"))),
+                            }
                         }
```
**Check:** a macro that expands to `macro_rules! inner { … }` followed by use of `inner!` works.

<a name="hir-9"></a>
### [HIR-9] [H] Builtins discard user code with side effects: `assert!`→`()`, `matches!`→`true`, `vec![..]` drops groups
**Where:** `crates/glyim-meta/src/expander/mod.rs:965-1015` (Vec :974-988, Matches :990-993, Assert :1009-1015)
`assert!(x.push(y));` expands to `()` — the argument never runs and no assertion fires. `vec![foo(1)]` silently loses the group argument.

**Fix:**
1. Vec — recurse into groups:
```diff
                 for tt in inner.iter() {
-                    if let TokenTree::Token(kind, text) = tt {
-                        …push element…
-                    }
+                    match tt {
+                        TokenTree::Token(kind, text) => { …push element… }
+                        TokenTree::Group(g) => {
+                            // re-emit delimiters + recurse
+                            out.push(TokenTree::Token(g.open_kind, g.open_text.clone()));
+                            out.extend(Self::expand_builtin_vec(&g.inner));  // same routine
+                            out.push(TokenTree::Token(g.close_kind, g.close_text.clone()));
+                        }
+                        _ => {}
+                    }
                 }
```
2. assert!/matches!: implement the real expansion (`if !(expr) { panic!("assertion failed") }`; `match (expr) { p => true, _ => false }`), or — if out of scope — emit a hard "unsupported builtin macro" error instead of discarding args. Never silently drop side effects.

<a name="hir-10"></a>
### [HIR-10] [C] Async `rewrite_for_poll` wraps the **wrong block's tail** (forward search vs documented last-allocated root)
**Where:** `crates/glyim-hir/src/lower/lower_async.rs:694-705` (contrast with `root_expr_id`, :154-166)
The crate's own invariant: children are pushed before parents, so the outermost block is the **last** `Expr::Block`. The forward `.find()` here returns the first-allocated (innermost) block. For `async fn f() -> i32 { if c { 1 } else { 2 } }` only the then-branch gets wrapped in `Poll::Ready`; the else path returns a non-Poll value — type error or miscompile.

**Fix:**
```diff
-    let root_block = (0..body.exprs.len())
-        .map(|i| ExprId::from_raw(i as u32))
-        .find(|&rid| matches!(body.exprs[rid], Expr::Block { .. }));
+    // Same convention as root_expr_id (:154-166): the outermost block is the
+    // LAST Expr::Block allocated.
+    let root_block = (0..body.exprs.len())
+        .rev()
+        .map(|i| ExprId::from_raw(i as u32))
+        .find(|&rid| matches!(body.exprs[rid], Expr::Block { .. }));
```
Better: factor `root_expr_id(body)` into a shared helper and call it from both sites so they cannot diverge.

**Check:** async fn with an if/else returning different values polls correctly (interp the state machine).

<a name="hir-11"></a>
### [HIR-11] [C] Multi-await state machine **drops statements between awaits**
**Where:** `crates/glyim-hir/src/lower/lower_async.rs:1466-1478` (Start Ready arm) and :1672-1718 (last-await arm)
(a) The Start arm's Ready path skips `pre_segments[1]` entirely — if `fut0` is Ready on first poll (the common case), statements between await 0 and 1 never execute. (b) `pre_segments` has `n+1` entries but the S-arm loop only consumes `1..n` — `pre_segments[n]` (statements between the last await and the tail) is **never emitted anywhere**.

**Fix:**
1. Start arm (:1466-1478): copy `pre_segments[1]` (with `arm_rename(0)`) into `ready_arm_body` before building `fut1`, mirroring the S_k arm (:1602-1611).
2. Last-await arm (:1672-1718): before computing the tail, copy `pre_segments[n]` (statements between the last await and the tail) into the Ready body.
3. Add the missing statement-run test: `let a = f().await; let b = g(a); let c = h().await; let d = k(c); d` — both `g` and `k` must run regardless of poll timing.

**Check:** interp an async fn with two awaits whose futures complete immediately — `g`/`k` side effects observed.

<a name="hir-12"></a>
### [HIR-12] [H] `mut` on pattern bindings is dropped — hardcoded `Mutability::Not`
**Where:** `crates/glyim-hir/src/lower/lower_pat.rs:49-53` (and :251-256)
The parser bumps `KwMut` inside `PatIdent` but lowering never reads it. Mutation of immutable bindings is silently accepted; closure/borrow classification is wrong.

**Fix:**
```diff
                 Some(pats.push(Pat::Binding {
                     name,
-                    mutability: Mutability::Not,
+                    mutability: if node.children_with_tokens().any(|c|
+                        c.as_token().map_or(false, |t| t.kind() == SyntaxKind::KwMut))
+                    { Mutability::Mut } else { Mutability::Not },
                     subpattern: subpat,
                 }))
```
**Check:** `let mut x = 0; x = 1;` typeck marks the binding mutable; borrowck tests on mutable captures pass.

<a name="hir-13"></a>
### [HIR-13] [M] `Item.is_unsafe` and `Item.visibility` hardcoded in `lower_fn_def`
**Where:** `crates/glyim-hir/src/lower/lower_item.rs:293-309`
The modifier scan detects const/async/extern but never `unsafe`; `visibility: Visibility::Inherited` is unconditional despite the parser attaching a `Visibility` node.

**Fix:**
```diff
+        let mut is_unsafe = false;
         for child in node.children_with_tokens() { …
+            if let NodeOrToken::Token(t) = child && t.kind() == SyntaxKind::KwUnsafe { is_unsafe = true; }
         }
         Some(Item { …, kind: ItemKind::Fn(FnItem {
             …
-            is_unsafe: false,
+            is_unsafe,
```
and reuse the def-map's `visibility_of_node` helper for `visibility`. Apply to all `lower_*` functions that build `Item`.

<a name="hir-14"></a>
### [HIR-14] [H] String unescaping applies `\\` **after** `\n`/`\t` — `\\n` becomes backslash+newline
**Where:** `crates/glyim-hir/src/lower/lower_expr.rs:1120-1128`
Source `"a\\nb"` (escaped backslash then n): the first replace matches the second backslash + n → `a\<newline>b`. Correct: `a\nb` literal.

**Fix (single left-to-right scanner — robust):**
```rust
fn unescape_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' { out.push(c); continue; }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some('\'') => out.push('\''),
            Some('"') => out.push('"'),
            Some('u') => { /* parse \u{…} if supported; else error diagnostic */ }
            Some(other) => { out.push('\\'); out.push(other); }  // keep as-is + diagnostic
            None => { out.push('\\'); }
        }
    }
    out
}
```
Replace the `.replace(…)` chain at :1120-1128 with `unescape_string(raw)`. Reuse `parse_char_literal`'s scanner if it can be generalized.

**Check:** `let s = "a\\nb";` — 4 chars (`a \ n b`); `"\n"` — 1 char.

<a name="hir-15"></a>
### [HIR-15] [H] Integer literal overflow silently becomes 0
**Where:** `crates/glyim-hir/src/lower/lower_expr.rs:1172-1183`
`i128::from_str_radix(…).unwrap_or(0)` / `s.parse::<i128>().unwrap_or(0)` — `99999999999999999999999999` becomes literal 0 with no error.

**Fix:**
```diff
-    (i128::from_str_radix(&s[2..], 16).unwrap_or(0), false)
+    match i128::from_str_radix(&s[2..], 16) {
+        Ok(v) => (v, false),
+        Err(_) => { diags.push(literal_too_large(span, s)); (0, false) }
+    }
```
(thread a `&mut Vec<GlyimDiagnostic>` into `lower_literal` — it's called from contexts that already have the diagnostics vec; also range-check against the suffix type: `256u8` errors "literal out of range for u8").

**Check:** `let x = 99999999999999999999999999;` produces "integer literal is too large".

<a name="hir-16"></a>
### [HIR-16] [M] `unreachable!()` panics on parser-recoverable input in block/binary lowering
**Where:** `crates/glyim-hir/src/lower/lower_expr.rs:176-179, 904-910`
Parser recovery nodes (e.g. `Error` nodes from `self.error(...)`) reach the match → compiler ICE instead of a diagnostic. `lower_bin_op_token`'s unreachable is reachable via the fallback path right above it (:832-850, which pushes an internal error then STILL hits the operator match).

**Fix:**
```diff
-            _ => unreachable!("parser produced a Block with unrecognized child kind: {:?}", child.kind()),
+            other => {
+                diagnostics.push(GlyimDiagnostic::internal_error(format!(
+                    "unrecognized block child {other:?}")));
+                continue;   // skip the node, keep compiling
+            }
```
and in the binary fallback (:832-850), `return None;` after pushing the internal error instead of continuing into `lower_bin_op_token`.

<a name="hir-17"></a>
### [HIR-17] [P] Array-repeat literal materializes up to 1M HIR nodes per occurrence
**Where:** `crates/glyim-hir/src/lower/lower_expr.rs:1978-1991`
`[0u8; 1_000_000]` allocates a 1M-element `Vec<ExprId>`; every later pass (typeck, collect_suspend_points, MIR lowering, drop elaboration) walks all million entries.

**Fix:** add a symbolic node:
```rust
// glyim-hir/src/lib.rs (Expr enum):
pub enum Expr {
    …
    ArrayRepeat { elem: ExprId, len: u128 },
}
```
Lower `[e; n]` to `ArrayRepeat` (the parser already keeps the `;` form — see the `count` variable at :1978). Update the ~6 match sites that iterate `Expr::Array(elems)` (grep `Expr::Array`) to also handle `ArrayRepeat` (typeck: element type checked once; MIR lowering already has `Rvalue::Repeat` — use it).

**Check:** `let z = [0u8; 1_000_000];` compiles in constant memory; MIR contains `Repeat`.

<a name="hir-18"></a>
### [HIR-18] [M] `struct_field_map` keyed by bare `Name`, only collected from top-level structs
**Where:** `crates/glyim-hir/src/lower/mod.rs:243-251` (used at lower_expr.rs:726-745)
Structs inside `mod` blocks never collected (no field reordering / missing-field check); same-named structs in different modules collide — last one wins.

**Fix:** populate inside `lower_mod_def`'s recursion and key by qualified path:
```rust
fn collect_struct_fields_recursive(node: &SyntaxNode, interner: &Interner,
    map: &mut HashMap<SmolStr, Vec<(Name, Span)>>, prefix: &str) {
    for child in node.children() {
        match child.kind() {
            SyntaxKind::StructDef => {
                if let Some((name, fields)) = collect_struct_fields(&child, interner) {
                    let qual = format!("{prefix}::{name}");
                    map.insert(qual, fields);
                }
            }
            SyntaxKind::ModuleDef => {
                let mname = …module name…;
                collect_struct_fields_recursive(&child, interner, map, &format!("{prefix}::{mname}"));
            }
            _ => {}
        }
    }
}
```
and look up with the resolved module prefix at the use site.

<a name="hir-19"></a>
### [HIR-19] [M] Synthesized async types are unqualified (`fFuture`/`fState`) at crate root — same-named async fns in different modules collide
**Where:** `crates/glyim-hir/src/lower/lower_async.rs:474-475` (also :1155-1156, item pushes :648-649, :1894-1896)
`mod a { async fn run() }` and `mod b { async fn run() }` both synthesize `runFuture` at top level → two definitions; wrapper return types resolve to whichever is found first.

**Fix:**
```diff
-    let future_name = format!("{}Future", fn_name_str);
+    let module_prefix = current_module_path()   // tracked during lowering, e.g. "a"
+        .map(|p| format!("{p}__"))
+        .unwrap_or_default();
+    let future_name = format!("{module_prefix}{}Future", fn_name_str);
```
Track the module path on the lowering context (it's already threaded for item lowering — reuse it); also register the synthesized items in the enclosing `ModItem.children` instead of only the root list.

<a name="hir-20"></a>
### [HIR-20] [M] `node_span` hardcodes `FileId::from_raw(1)` — all HIR spans claim file 1
**Where:** `crates/glyim-hir/src/lower/mod.rs:122-127`
Multi-file crates (modules from `name.g` files) get wrong-file spans for every HIR-derived span; def-map uses `FileId::BOGUS` — the layers disagree.

**Fix:** thread the real FileId through lowering:
```diff
 pub(crate) fn node_span(node: &SyntaxNode) -> Span {
     let range = node.text_range();
     …
-    Span::new(FileId::from_raw(1), lo, hi, SyntaxContext::ROOT)
+    Span::new(CURRENT_FILE_ID.with(|f| f.get()), lo, hi, SyntaxContext::ROOT)
 }
```
Set a `thread_local! { static CURRENT_FILE_ID: Cell<FileId> }` in `lower_crate`'s entry from the parsed root's file id (it's known when the root is parsed — `lower_crate_for_pipeline(root, interner, file_id)`), restore on exit.

**Check:** multi-module project — diagnostics point into the right file.

<a name="hir-21"></a>
### [HIR-21] [H] `use`/glob imports silently overwrite existing bindings — no conflict detection
**Where:** `crates/glyim-def-map/src/lib.rs:369-390` (glob), :707-765 (simple), :130-151 (`declare`)
`ItemScope::declare` uses `IndexMap::insert` (replace). Glob `use foo::*;` rebinds a name the module defines itself; two `use a::f; use b::f;` are last-wins; a use-collision with a local definition is accepted (Rust: E0252/E0659).

**Fix:**
```diff
     pub fn declare(&mut self, name: Name, id: LocalDefId, vis: Visibility, span: Span, ns: Namespace) {
-        self.values.insert(name, (id, vis, span));
+        if let Some(existing) = self.values.get(&name) {
+            if existing.0 != id {
+                // Import conflicts are errors; glob-vs-explicit: explicit wins.
+                self.pending_conflicts.push(ImportConflict { name, existing: existing.0, new: id, span });
+                return;
+            }
+        }
+        self.values.insert(name, (id, vis, span));
```
Add a `pending_conflicts: Vec<ImportConflict>` drained by def-map's `finish()` into diagnostics. Give glob imports a `from_glob: bool` and let explicit declarations silently override globs (glob-vs-glob = ambiguity error only on use).

**Check:** `use a::f; use b::f;` errors E0252-style; `use foo::*;` doesn't shadow a local `f`.

<a name="hir-22"></a>
### [HIR-22] [M] FFI imports force-registered in crate root regardless of module; first declaration wins even with different signatures
**Where:** `crates/glyim-def-map/src/lib.rs:1046-1067`
`mod a { extern "C" { fn f(x: i32); } } mod b { extern "C" { fn f(x: u64); } }` — both hoisted root-level Public; module b's calls type-check against a's signature. Private extern blocks become crate-wide.

**Fix:**
```diff
-                    if !scope.values.contains_key(&name) {
-                        scope.declare(name, id, Visibility::Public, span, Namespace::Values);
-                    }
+                    // Declare in the OWNING module first:
+                    modules[owner].scope.declare(name, id, vis, span, Namespace::Values);
+                    // Root-level fallback only when nothing else declared it:
+                    if !scope.values.contains_key(&name) {
+                        scope.declare(name, id, Visibility::Public, span, Namespace::Values);
+                    }
```
and when a root-level symbol already exists with a *different* def-id, emit a diagnostic instead of silently keeping the first.

<a name="hir-23"></a>
### [HIR-23] [H] `compare_eq` has no float or mixed-numeric arms — const `1.5 == 1.5` folds to **false**
**Where:** `crates/glyim-const-eval/src/eval.rs:852-862`
Float comparisons fall to `_ => false`. Mixed Int/Uint also silently false. `compare_lt` (:864-882) at least errors — inconsistent.

**Fix:**
```diff
             (ConstValue::Unit, ConstValue::Unit) => true,
+            (ConstValue::FloatBits(a, _), ConstValue::FloatBits(b, _)) => {
+                // IEEE equality: preserves NaN != NaN
+                f64::from_bits(*a) == f64::from_bits(*b)
+            }
+            (ConstValue::Int(a, _), ConstValue::Uint(b, _))
+            | (ConstValue::Uint(b, _), ConstValue::Int(a, _)) => {
+                // compare in the widest common domain
+                *a == *b as i128
+            }
-            _ => false,
+            _ => return Err(ConstEvalError::new("incomparable const operands")),
```
**Check:** `const X: bool = 1.5 == 1.5;` → true; `const Y: bool = 1 == 1u32;` → true.

<a name="hir-24"></a>
### [HIR-24] [H] f32 const arithmetic computed in f64 bit-space — garbage for f32 values produced by casts
**Where:** `crates/glyim-const-eval/src/value.rs:172-177` (also :229-238)
`Literal::Float` stores f64 bits, but `eval_cast` to f32 stores f32 bits; arithmetic unconditionally reinterprets with `f64::from_bits`. An f32-typed value carrying real f32 bits (0x3F800000) becomes ~5.3e-315; `checked_div`'s zero path stores an f64-infinity bit pattern tagged F32.

**Fix:**
```diff
             (ConstValue::FloatBits(a, ty_a), ConstValue::FloatBits(b, ty_b)) if ty_a == ty_b => {
-                Some(ConstValue::FloatBits(
-                    (f64::from_bits(*a) + f64::from_bits(*b)).to_bits(), *ty_a))
+                let bits = match ty_a {
+                    FloatTy::F32 => ((f32::from_bits(*a as u32) + f32::from_bits(*b as u32)).to_bits()) as u64,
+                    FloatTy::F64 => (f64::from_bits(*a) + f64::from_bits(*b)).to_bits(),
+                };
+                Some(ConstValue::FloatBits(bits, *ty_a))
             }
```
Decide ONE representation (recommendation: always store f64 bits internally; convert to f32 only in `eval_cast` and `validate_range`) and apply it to every FloatBits site — grep `FloatBits(` in value.rs and eval.rs (also fix `checked_div`'s infinity constant).

**Check:** `const A: f32 = 1.0; const B: f32 = A + A;` — B == 2.0 (currently garbage).

<a name="hir-25"></a>
### [HIR-25] [M] Const struct-pattern matching zips fields by position, ignoring names
**Where:** `crates/glyim-const-eval/src/eval.rs:1120-1134`
`match p { Point { y, x } => … }` compares pattern `y` against the value's `x`. Rest patterns rejected by count mismatch.

**Fix:**
```diff
             Pat::Struct { fields, .. } => {
                 if let ConstValue::Struct(vals) = value {
-                    if fields.len() != vals.len() { return Ok(false); }
-                    for ((_, pat_id), (_, val)) in fields.iter().zip(vals.iter()) {
-                        if !self.pattern_matches(pat_id, val)? { return Ok(false); }
+                    for (pat_name, pat_id) in fields.iter() {
+                        let val = match vals.iter().find(|(n, _)| n == pat_name) {
+                            Some((_, v)) => v,
+                            None => return Ok(false),   // field absent
+                        };
+                        if !self.pattern_matches(pat_id, val)? { return Ok(false); }
                     }
```
**Check:** out-of-order named fields in a const match bind correctly.

<a name="hir-26"></a>
### [HIR-26] [M] `break`/`continue` signal leaks through blocks — trailing statements still evaluate; stale flag survives
**Where:** `crates/glyim-const-eval/src/eval.rs:606-617` (signal), :1196-1213 (`eval_block`)

**Fix:**
```diff
         self.env.push(HashMap::new());
         for stmt_id in stmts {
             self.evaluate_at_depth(*stmt_id, depth)?;
+            if self.loop_control.is_some() { break; }   // honor break/continue immediately
         }
```
and have every loop driver (`eval_while`/`eval_loop`/`eval_for`) set `self.loop_control = None;` on entry so a stale signal can't leak between constructs.

**Check:** `loop { { break; } side_effect(); }` — side effect does not run in const eval.

<a name="hir-27"></a>
### [HIR-27] [M] Shifts silently wrap while other const arithmetic overflows — inconsistent policy
**Where:** `crates/glyim-const-eval/src/eval.rs:969-1007`
`const X: i32 = 1 << 40;` folds to 0; `*b as u32` also truncates a 128-bit shift amount.

**Fix:**
```diff
             (ConstValue::Int(a, ty), ConstValue::Int(b, _)) => {
-                Ok(ConstValue::Int(a.wrapping_shl(*b as u32), *ty))
+                let width = int_ty_bit_width(ty) as u32;
+                if *b < 0 || *b >= width as i128 {
+                    return Err(ConstEvalError::new("attempt to shift with overflow"));
+                }
+                let shifted = a.checked_shl(*b as u32).ok_or_else(|| ConstEvalError::new("shift overflow"))?;
+                validate_range(ConstValue::Int(shifted, *ty))   // existing range checker
             }
```
(mirror for Uint and Shr).

<a name="hir-28"></a>
### [HIR-28] [M] Array index bounds check truncates the u128 index before comparing
**Where:** `crates/glyim-const-eval/src/eval.rs:437-461`
`ARR[2^64]` truncates to 0 → silently returns element 0.

**Fix:**
```diff
-                    if (idx as usize) < arr.len() {
+                    if idx <= usize::MAX as u128 && (idx as usize) < arr.len() {
```
(and for the slice arm below it, same guard).

<a name="hir-29"></a>
### [HIR-29] [C] `match` lowers the scrutinee **twice** for non-enum dispatch — duplicated side effects, split temps
**Where:** `crates/glyim-lower/src/lower_rvalue.rs:1408, 1428-1440`
The comment right above (:1420-1427) explicitly warns that lowering twice "allocates two distinct temps … a silent miscompile" — and the `else` branch does exactly that: `lower_expr_to_place(scrutinee)` runs call #1 into the temp used for arm binding, then `lower_expr_to_operand(scrutinee)` runs it **again** for `SwitchInt`. `match counter.next() { 0 => … }` advances the counter twice.

**Fix:**
```diff
-        let full_scrut_op = if enum_dispatch {
-            glyim_mir::Operand::Copy(scrutinee_place.clone())
-        } else {
-            self.lower_expr_to_operand(scrutinee)
-        };
+        // NEVER re-lower the scrutinee. Reuse the materialized place for dispatch too.
+        let full_scrut_op = glyim_mir::Operand::Copy(scrutinee_place.clone());
```
**Check:** run-pass: `let mut n = Counter::new(); match n.next() { 1 => exit(0), _ => exit(1) }` — exactly one `next()` call (add a call counter to the test).

<a name="hir-30"></a>
### [HIR-30] [C] `break <value>` is evaluated and then **discarded** — loop break values dropped
**Where:** `crates/glyim-lower/src/lower_rvalue.rs:820-839`
`let x = loop { break compute(); };` lowers `compute()` to a temp that is immediately discarded (`let _ =`); `LoopInfo` has no break-value slot; the Loop arm returns Unit — `x` is never assigned.

**Fix:**
1. `LoopInfo` (in `crates/glyim-lower/src/lower.rs`) gains `break_place: LocalIdx` — allocate at loop entry with the loop's result type.
2. Break arm:
```diff
             thir::ExprKind::Break { value } => {
                 if let Some(val_expr) = value {
-                    let _ = self.lower_expr_to_rvalue(val_expr);
+                    let rv = self.lower_expr_to_rvalue(val_expr);
+                    let break_place = self.loop_stack.last()
+                        .map(|i| i.break_place)
+                        .expect("break outside loop is caught by typeck");
+                    self.emit_assign(Place::new(break_place), rv);
                 }
                 let target_bb = self.loop_stack.last().map(|info| info.break_bb);
```
3. The `Loop` arm at `exit_bb` yields `Operand::Move(break_place)` instead of the Unit constant (initialize `break_place` to `Unit`/`MaybeUninit` when the loop can break without a value).

**Check:** run-pass `let x = loop { break 5; }; assert(x == 5)` (interp or bytecode).

<a name="hir-31"></a>
### [HIR-31] [C] Closure `ByRef`/`ByRef(Mut)` captures are copied by value — no aliasing
**Where:** `crates/glyim-lower/src/lower_rvalue.rs:888-901`
A `&mut` closure (`let mut c = 0; || { c += 1 }`) increments a snapshot copy — `c` never changes; `FnMut` semantics are wrong. `Copy` of non-Copy captured locals is also an invalid MIR read.

**Fix:**
```diff
-                        CaptureKind::ByRef(glyim_core::primitives::Mutability::Not)
-                        | CaptureKind::ByRef(glyim_core::primitives::Mutability::Mut) => {
-                            glyim_mir::Operand::Copy(glyim_mir::Place::new(capture_local))
-                        }
+                        CaptureKind::ByRef(mutability) => {
+                            // Materialize a reference into the captured local:
+                            let ref_local = self.new_local(ref_ty_of(capture_local_ty, mutability));
+                            self.emit_assign(Place::new(ref_local), glyim_mir::Rvalue::Ref(
+                                glyim_mir::Place::new(capture_local), borrow_kind(mutability)));
+                            glyim_mir::Operand::Move(Place::new(ref_local))
+                        }
```
The environment field type becomes `&T`/`&mut T`; use sites must auto-deref (the Field-on-ref machinery at :1063-1068 already exists — reuse it when the closure body reads/writes captured fields).

**Check:** `let mut c = 0; let mut f = || { c += 1 }; f(); f(); assert(c == 2)` in the interpreter.

<a name="hir-32"></a>
### [HIR-32] [H] For-loop desugar: unresolved-iterator fallback silently runs the body once; iterator never dropped at exit; StorageLive re-executes per iteration
**Where:** `crates/glyim-lower/src/lower_rvalue.rs:442-624` (fallback :605-623; StorageLive :500-503, :532-535, :578-581)

**Fix (3 parts):**
1. Replace the fallback with a hard error:
```diff
                     None => {
-                        // Simplified fallback: execute the body once and break.
-                        self.current_block = Some(header_bb);
-                        let _ = self.lower_expr_to_rvalue(body);
+                        self.diagnostics.push(GlyimDiagnostic::type_error(
+                            "for-loop over a type with no resolvable `Iterator::next`"));
+                        self.current_block = Some(exit_bb);
```
2. Hoist `StorageLive(iter_local)` (and ref/option temporaries) into a pre-header block that runs once.
3. On `exit_bb` (which is also `break_bb`), emit `StorageDead(iter_local)` + `Drop(iter_local)` before the merge.

**Check:** `for x in v.into_iter() {}` followed by more code — the iterator drops at loop exit (interp drop counter), and a custom type without `next` errors instead of silently running once.

<a name="hir-33"></a>
### [HIR-33] [P] `discover_mono_roots` re-flattens the entire CST per item — O(items × tokens)
**Where:** `crates/glyim-lower/src/discovery.rs:32-65` and :85-122
`has_attr_in_span` clones every token of the whole tree into a fresh `Vec` per call (2 calls per fn + 1 per static).

**Fix:** scan attributes once:
```rust
// one pass over the tree at discovery start:
let mut attrs_by_span: Vec<(Span, String)> = Vec::new();
walk_attr_nodes(&root, &mut attrs_by_span);   // collect (item_span, attr_name) pairs
// per item: attrs_by_span.iter().any(|(s, n)| s.intersects(item.span) && n == "start")
```
Also drop the global `collect_tokens` per call entirely.

<a name="hir-34"></a>
### [HIR-34] [H] Every proc-macro token leaks a `CString` — `mem::forget` per token per invocation
**Where:** `crates/glyim-proc-macro/src/lib.rs:351-377`
The comment claims "kept alive for the duration of the call" but `forget` leaks permanently, for every token of every invocation.

**Fix:**
```diff
                 for (kind, text) in input {
-                    let ctext = CString::new(text.as_str()).unwrap_or_default();
-                    let pm_text = PmStr { ptr: ctext.as_ptr() as *const u8, len: text.len() as u32 };
-                    pm_ts_push(&mut in_ts, PmToken { kind: *kind as u16, text: pm_text });
-                    std::mem::forget(ctext);
+                    // keep every CString alive on the stack until the call returns:
+                    owned.push(CString::new(text.as_str()).unwrap_or_default());
                 }
+                // second pass now that the buffers are stable:
+                for (tok, ctext) in input.iter().zip(&owned) {
+                    let pm_text = PmStr { ptr: ctext.as_ptr() as *const u8, len: tok.1.len() as u32 };
+                    pm_ts_push(&mut in_ts, PmToken { kind: tok.0 as u16, text: pm_text });
+                }
+                let out = (entry)(&mut in_ts, &mut out_ts);
+                drop(owned);   // freed AFTER the dylib call returns
```
Declare `let mut owned: Vec<CString> = Vec::with_capacity(input.len());` before the loop.

**Check:** run a proc-macro 1000 times under a memory counter — RSS flat (currently grows linearly).

<a name="hir-35"></a>
### [HIR-35] [P] Dead per-call type query and Debug-formatting in mono's hot loop
**Where:** `crates/glyim-lower/src/mono.rs:360-370` (and :161)
For every call terminator, the first argument's type is resolved and immediately discarded (`let _ =`); line 161 builds a `format!("{:?}", item)` string per mono item even when codegen mangles separately.

**Fix:** delete the `arg0` block (:360-370) — the `has_param` guard below is the real check; compute the symbol lazily (only when `symbol_name(item)` is actually requested) or cache it in the mono item.

---

## 7. `glyim-codegen-llvm` + `glyim-typeck` check_expr (19 findings)

The LLVM backend (`lower.rs`, 4437 lines) contains a systematic **signedness gap** — unsigned comparisons, widening casts, and float↔int conversions are all wrong. These are the highest-value fixes in the whole report because they silently miscompile ordinary arithmetic.

<a name="ll-1"></a>
### [LL-1] [C] All integer `<`/`>`/`<=`/`>=` comparisons use **signed** LLVM predicates
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:1942-2088` (`SLT`:1946, `SGT`:1983, `SLE`:2020, `SGE`:2057 — the only unsigned predicate anywhere is `ULT` at :1634)
`operand_ty` is available (used for `Div` :1796, `Rem` :1833, `Shr` :2194) but comparisons never consult it. `let a: u32 = 3_000_000_000; a > 4` → `SGT` on i32 → false. Every `usize`/`u64` comparison ≥ 2^63 miscompiles.

**Fix (4 arms, same pattern):**
```diff
             BinOp::Lt => {
                 if l.is_int_value() && r.is_int_value() {
+                    let signed = self.is_signed_int_ty(operand_ty);
+                    let pred = if signed { IntPredicate::SLT } else { IntPredicate::ULT };
                     self.builder
                         .build_int_compare(
-                            IntPredicate::SLT,
+                            pred,
                             l.into_int_value(),
                             r.into_int_value(),
                             "lt",
```
Repeat with `SGT/UGT`, `SLE/ULE`, `SGE/UGE` for `Gt`/`LtEq`/`GtEq`. (`self.is_signed_int_ty` already exists — used by the Div arm.)

**Check:** run-pass `fn main() { let a: u32 = 3_000_000_000; if a > 4 { exit(0); } exit(1); }` exits 0.

<a name="ll-2"></a>
### [LL-2] [C] `IntToInt` widening **always zero-extends** — `i8 as i32` with a negative value gives 255
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:2294-2323`
No source-signedness check; every widening emits `build_int_z_extend`.

**Fix:** thread the source `Ty` into `lower_cast` (available at the `Rvalue::Cast` call site as the operand's type):
```diff
                     } else if target_bits > src_bits {
-                        self.builder
-                            .build_int_z_extend(int_val, target_llvm_ty.into_int_type(), "zext")
+                        if self.is_signed_int_ty(src_ty) {
+                            self.builder
+                                .build_int_sign_extend(int_val, target_llvm_ty.into_int_type(), "sext")
+                        } else {
+                            self.builder
+                                .build_int_z_extend(int_val, target_llvm_ty.into_int_type(), "zext")
+                        }
```
**Check:** run-pass `let a: i8 = -1; let b: i32 = a as i32; if b == -1 { exit(0); } exit(1);`.

<a name="ll-3"></a>
### [LL-3] [H] `f as uN` always lowers to `fptosi` — values ≥ 2^63 are LLVM poison
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:2330-2350`

**Fix:**
```diff
             CastKind::FloatToInt => {
                 if val.is_float_value() && target_llvm_ty.is_int_type() {
                     let float_val = val.into_float_value();
-                    self.builder
-                        .build_float_to_signed_int(float_val, target_llvm_ty.into_int_type(), "fptosi")
+                    if matches!(self.ty_kind_of(target_ty), TyKind::Uint(_)) {
+                        self.builder
+                            .build_float_to_unsigned_int(float_val, target_llvm_ty.into_int_type(), "fptoui")
+                    } else {
+                        self.builder
+                            .build_float_to_signed_int(float_val, target_llvm_ty.into_int_type(), "fptosi")
+                    }
```
**Check:** `let x: f64 = 1e19; let y: u64 = x as u64;` — y is large-positive, not garbage.

<a name="ll-4"></a>
### [LL-4] [H] `uN as f` always lowers to `sitofp` — `u64::MAX as f64` gives -2.0
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:2352-2371`

**Fix:**
```diff
-                    self.builder
-                        .build_signed_int_to_float(int_val, target_llvm_ty.into_float_type(), "sitofp")
+                    if matches!(self.ty_kind_of(src_ty), TyKind::Uint(_)) {
+                        self.builder
+                            .build_unsigned_int_to_float(int_val, target_llvm_ty.into_float_type(), "uitofp")
+                    } else {
+                        self.builder
+                            .build_signed_int_to_float(int_val, target_llvm_ty.into_float_type(), "sitofp")
+                    }
```
**Check:** `let x: u64 = u64::MAX; let y: f64 = x as f64;` ≈ 1.8e19, not -2.0.

<a name="ll-5"></a>
### [LL-5] [C] Array index local loaded as `i64` regardless of the local's type — adjacent stack garbage drives the GEP
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:773-780`
`let i: i32 = 1; arr[i]` — the slot is a 4-byte `i32` alloca, but this performs an 8-byte `i64` load pulling 4 adjacent bytes into the high bits → OOB address / segfault, misaligned.

**Fix:**
```diff
                 ProjectionElem::Index(local_idx) => {
                     let index_ptr = self.get_local_ptr(*local_idx);
-                    let i64_ty = self.llvm_int_type(64);
-                    let index_val = self.builder
-                        .build_load(i64_ty, index_ptr, "index_load").expect("index load failed")
-                        .into_int_value();
+                    let local_ty = self.body.locals[*local_idx].ty;
+                    let local_llvm_ty = self.llvm_type_for_ty(local_ty);
+                    let raw = self.builder
+                        .build_load(local_llvm_ty, index_ptr, "index_load").expect("index load failed")
+                        .into_int_value();
+                    let i64_ty = self.llvm_int_type(64);
+                    let index_val = if self.is_signed_int_ty(local_ty) {
+                        self.builder.build_int_sign_extend(raw, i64_ty, "idx_sext")
+                    } else {
+                        self.builder.build_int_z_extend(raw, i64_ty, "idx_zext")
+                    };
```
**Check:** run-pass indexing with an `i32` loop variable over `[i32; 8]` — correct elements, no crash.

<a name="ll-6"></a>
### [LL-6] [C] Out-of-order struct literal fields are silently written to the wrong fields
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:1465-1501` (+ check_expr.rs:1991-2010, glyim-lower/lower_rvalue.rs:794-802)
`build_layout_aggregate` maps *operand position* → *declaration-order offset*, but typeck preserves the literal's source order and nobody re-sorts. `Point { y: 7, x: 2 }` writes 7 into `x`, 2 into `y` — silent value swap through the whole pipeline.

**Fix (single authoritative site — typeck):** in `check_expr.rs`'s `Expr::Struct` arm (:1991-2010), reorder fields to declaration order using the ADT's field index:
```rust
// after resolving the ADT:
let mut ordered: Vec<(FieldIdx, Ty, ExprId)> = Vec::with_capacity(thir_fields.len());
for f in adt_def.fields.iter() {
    if let Some((_, ty, expr)) = thir_fields.iter().find(|(n, _, _)| *n == f.name) {
        ordered.push((f.idx, *ty, *expr));
    }
}
// error on missing field (already checked); use `ordered` when building the
// THIR Aggregate so operand i == declaration field i.
```
**Check:** run-pass with `Point { y: 7, x: 2 }` — prints x=2, y=7.

<a name="ll-7"></a>
### [LL-7] [C] Value-level allocas emitted in the *current* block — loops grow the stack per iteration
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:1325-1330` (also :3471-3476, :3504-3511, :3523-3531, :1081-1084)
`build_layout_aggregate`'s `"agg_tmp"`, `lower_call`'s `"sret"`/`"arg"`/`"arg_cast"`, and `place_ptr`'s `subslice_tmp` all `build_alloca` at the current insertion point — including loop bodies. LLVM does not hoist non-entry-block allocas → stack exhaustion proportional to trip count.

**Fix:** hoist all value-level allocas to the entry block:
1. In the function prologue (before the parameter-copy code at :4183-4185), create `let entry_builder = self.builder.` — concretely keep a second `Builder` positioned at the entry block's first insertion point: `let mut entry_builder = self.context.create_builder(); entry_builder.position_at_end(entry_bb);` and stash it on the lowering ctx.
2. Replace all five `self.builder.build_alloca(...)` value-level call sites with `self.entry_builder.build_alloca(...)` (keep the local-decl allocas where they are — they already run in the entry block).
**Check:** `while cond { let p = Point { a, b }; … }` with 1M iterations — RSS/stack flat.

<a name="ll-8"></a>
### [LL-8] [H] `invoke` + sret appends the result store **into the target block after its terminator**
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:3705-3719`
Re-positioning onto the call's MIR target block and emitting load/store there is invalid when that block was already emitted with a terminator (back-edges) — LLVM verifier ICE.

**Fix:** never reposition onto an arbitrary MIR block; create a continuation block (the `drop_cont` pattern at :2761 already does this):
```diff
         if use_invoke && let Some(target_bb) = target {
-            let target_block = self.bb_map.get(target_bb).unwrap();
-            self.builder.position_at_end(*target_block);
+            // Land in a fresh continuation block; branch to the real target at the end.
+            let cont = self.context.append_basic_block(function, "call_cont");
+            self.builder.position_at_end(cont);
         }
         if is_sret { …existing load/store into the continuation… }
+        if use_invoke {
+            let target_block = *self.bb_map.get(target_bb.unwrap()).unwrap();
+            self.builder.build_unconditional_branch(target_block);
+        }
```
**Check:** a loop body whose last statement is a struct-returning call compiles and `opt -verify` passes.

<a name="ll-9"></a>
### [LL-9] [C] Function-definition parameter mapping ignores `PassMode::Ignore` (ZST args) — store to null pointer, args never initialized
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:4192-4231` (attributes :4064-4101)
`fn f(u: (), x: i32)` lowers as `fn(i32)`; the prologue maps MIR arg 1 (unit) → LLVM param 0 (`x`) and stores into the unit slot — which `alloc_local` set to a **null pointer** (:221-227) → store to address 0 at entry; arg 2 maps to `params.get(1) = None` and is skipped.

**Fix:** build a filtered mapping once:
```rust
// in lowering_ctx construction:
let arg_param_map: Vec<Option<u32>> = fn_abi.args.iter().scan(0u32, |llvm_idx, arg| {
    let m = if matches!(arg.mode, PassMode::Ignore) { None } else { let m = Some(*llvm_idx); *llvm_idx += 1; Some(m) };
    Some(m)
}).collect();
```
Prologue:
```diff
     for i in 1..=body.arg_count {
         let local_idx = LocalIdx::from_raw(i as u32);
-        let param_idx = if lowering_ctx.is_sret { i } else { i - 1 };
-        if let Some(param_val) = params.get(param_idx as usize) {
+        let mir_arg = i - 1; // MIR arg 0-based
+        if let Some(Some(param_idx)) = lowering_ctx.arg_param_map.get(mir_arg) {
+            let param_idx = *param_idx + if lowering_ctx.is_sret { 1 } else { 0 }; // skip hidden sret ptr
+            if let Some(param_val) = params.get(param_idx as usize) {
```
Attribute loop (:4064-4101): `param_idx += 1` **only** for non-`Ignore` modes (mirror the call-site loop at :3701-3703).

**Check:** run-pass `fn f(u: (), x: i32) -> i32 { x }` returns the right value (currently segfaults/garbage).

<a name="ll-10"></a>
### [LL-10] [C] `&&`/`||` lowered as **eager** bitwise and/or — no short-circuit
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:2090-2123` (semantics fixed upstream: glyim-hir lower_expr.rs:896-897 → glyim-lower lower_rvalue.rs:214-218)
`i < arr.len() && arr[i] == x` evaluates `arr[i]` even when the bounds check fails — OOB read. There is no `LogicalOp` construct anywhere in THIR/MIR.

**Fix (desugar in glyim-lower, where `&&`/`||` are seen as `Rvalue::BinaryOp`):** in `crates/glyim-lower/src/lower_rvalue.rs:214-218`, intercept:
```rust
match (op, ty) {
    (BinOp::And, t) if is_bool(t) => {
        // result = lhs ? rhs : false
        let lhs_bb = self.current_block();
        let rhs_bb = self.new_block("and_rhs");
        let join_bb = self.new_block("and_join");
        let res = self.new_local(Ty::BOOL);
        self.emit_switch(lhs, [(1, rhs_bb)], join_bb);
        self.current_block = Some(rhs_bb);
        self.emit_assign(res_place, rhs_rvalue);           // evaluate RHS only if lhs true
        self.emit_goto(join_bb);
        self.current_block = Some(join_bb);
        // phi is not needed: write false on the fallthrough edge before joining
        return Ok(Rvalue::Use(Operand::Copy(res_place)));
    }
    …
}
```
(Same for `Or` with true.) Update the interpreter's `BinOp::And`/`Or` arms to match (short-circuit is invisible to the interpreter's value-level eval except for side effects — document the divergence or evaluate rhs lazily there too).

**Check:** run-pass `if i < len && arr[i] == 0` with `len == 0` — no OOB (interp with bounds instrumentation or run under ASan).

<a name="ll-11"></a>
### [LL-11] [C] `resolve_trait_method_fn` commits unification side effects while probing impls
**Where:** `crates/glyim-typeck/src/check_expr.rs:3355-3368`
The probe unifications permanently bind inference vars — the first impl whose self type unifies with a fresh `?T` (e.g. `impl Display for i32` binds `?T := i32`) corrupts the receiver's inference var for every later use. `collect_for` (:2837-2844) does this correctly with snapshots; this site doesn't.

**Fix:**
```diff
+                    let inf_snap = self.infer.snapshot();
+                    let diag_len = self.diagnostics.len();
                     let self_matches = recv_steps.iter().any(|&rt| { …unify probes… });
+                    if !self_matches {
+                        self.infer.rollback_to(inf_snap);
+                        self.diagnostics.truncate(diag_len);
+                    }
```
**Check:** `cargo test -p glyim-typeck` — the for-loop item-type tests stay green; add a case where the first impl in the crate is an unrelated one.

<a name="ll-12"></a>
### [LL-12] [H] Raw-pointer intrinsic methods (`is_null`, `add`, `addr`) type-check but produce "call the pointer" THIR — codegen ICE
**Where:** `crates/glyim-typeck/src/check_expr.rs:2689-2725` (fallback :1522-1535)
These arms return the right *type* with `dispatch = None`; the fallback builds `Call { func: recv_expr }` — a call whose callee is the pointer value; `lower_call` errors "expected function pointer or closure type" (lower.rs:3452-3456) on code typeck accepted.

**Fix:** register them as real synthetic builtins (like `try_builtin_method` does):
```rust
// in the RawPtr arm:
match mname.as_str() {
    "is_null" | "is_not_null" => {
        let builtin = BuiltinFn::RawPtrIsNull { negate: mname == "is_not_null" };
        return (Ty::BOOL, Some(MethodDispatch::Builtin(builtin)));
    }
    "add" | "offset" => {
        return (recv_ty, Some(MethodDispatch::Builtin(BuiltinFn::PtrAdd)));
    }
    …
}
```
and add matching arms to `try_lower_builtin_intrinsic` in glyim-lower (emit `ptrtoint`/`inttoptr`+`add` in LLVM; error in the bytecode backend until supported). If a method can't be dispatched, `resolve_method_call` must emit a diagnostic instead of returning `None`.

<a name="ll-13"></a>
### [LL-13] [H] Builtin-method fallback accepts *any* ADT receiver — wrong-impl dispatch
**Where:** `crates/glyim-typeck/src/check_expr.rs:2551-2567`
When a user ADT's own lookup misses, the fallback probes builtin `Result`/`Option` tables and accepts if the receiver "is an Adt" — a user `struct Wrapper<T>(T)` with no `unwrap` gets `Result`'s builtin `unwrap` (garbage layout dispatch).

**Fix:**
```diff
                 for cand in candidates {
                     if let Some(hit) = self.ctx.lookup_builtin_method(AdtId::from_raw(cand), method_name) {
-                        if matches!(self.ctx.ty_kind(step_ty), TyKind::Adt(_, _)) {
+                        // Only accept if the receiver IS that builtin (by name):
+                        let recv_is_builtin = matches!(
+                            (self.ctx.ty_kind(step_ty), cand),
+                            (TyKind::Adt(_, _), 1006) if self.adt_name_is(step_ty, "Option")
+                        ) || matches!(
+                            (self.ctx.ty_kind(step_ty), cand),
+                            (TyKind::Adt(_, _), 1007) if self.adt_name_is(step_ty, "Result")
+                        );
+                        if recv_is_builtin {
                             found = Some(hit);
                             break;
                         }
```
(add an `adt_name_is` helper reading the ADT's name from the def-map/interner; 1006/1007 are the existing builtin ADT ids used above the excerpt.)

**Check:** `struct W<T>(T); fn f(w: W<i32>) { w.unwrap(); }` errors "no method `unwrap`".

<a name="ll-14"></a>
### [LL-14] [H] `self.expr_cache.clear()` in the closure arm wipes type results for the whole enclosing body
**Where:** `crates/glyim-typeck/src/check_expr.rs:2103-2114` (consumed at check_stmt.rs:318-325)
For any function containing a closure literal, all expressions checked *before* it return no type from `expr_ty` (LSP/IDE queries) and everything after is re-resolved from scratch.

**Fix:**
```diff
-                self.expr_cache.clear();
+                // Re-check only the closure subtree (its VarRefs must resolve in
+                // the closure scope), keeping the rest of the body's cache.
+                let closure_body = body_id_of_closure;
+                let ids: Vec<ExprId> = collect_subtree_ids(self.body, closure_body);
+                for id in ids { self.expr_cache.remove(&id); }
```
(`collect_subtree_ids` = the same walker `check_stmt.rs::guard_subtree_ids` already implements for closures.)

**Check:** typeck a fn with 100 statements then a closure — `expr_ty` still returns types for the first 100.

<a name="ll-15"></a>
### [LL-15] [M] Closure call: `fn_sig.inputs.len() - args.len()` can underflow
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:3432`
Typeck's arity check only pushes a diagnostic and *continues*; release wraps to `usize::MAX` → OOB extract loop → ICE.

**Fix:**
```diff
-                let capture_count = (fn_sig.inputs.len() as usize) - args.len();
+                let capture_count = fn_sig.inputs.len().saturating_sub(args.len());
```

<a name="ll-16"></a>
### [LL-16] [P] Method resolution re-scans all HIR items per deref-step, per call — plus `std::env::var` in hot loops
**Where:** `crates/glyim-typeck/src/check_expr.rs:2744-2746, 2918-2927` (and :1208-1228)
O(method_calls × 13 steps × all_items × resolve_type_ref); `std::env::var("GLYIM_DBG_CAND_PUSH")` (lock + env scan) executes *per matched candidate*.

**Fix:** (1) build once per body a `Name → Vec<impl index>` map of method names from all impls (same pre-pass shape as SOLVE-20); (2) read debug env vars once into `static DBG: OnceLock<DebugFlags>` at typeck entry.

<a name="ll-17"></a>
### [LL-17] [P] Layout computer reconstructed (with a `String` clone) per projection/alloca/call — no caching anywhere
**Where:** `crates/glyim-codegen-llvm/src/lower.rs:495-497` (also :237, :839-840, :1317, :1524, :2812, :3458)
`FullLayoutComputer::new(self.ty_ctx, self.target_info.clone())` heap-allocates per construction; `layout_of` recursively recomputes with zero memoization (no cache exists anywhere in glyim-layout).

**Fix:** (1) construct one `FullLayoutComputer` per `LoweringCtx` (immutable — store it); (2) add `cache: RefCell<HashMap<Ty, Layout>>` inside the computer keyed by interned `Ty`, populated in `layout_of`'s entry. Apply the same to `crates/glyim-codegen`'s provider (pairs with RT-17).

**Check:** benchmark compiling a struct-heavy crate before/after — expect a large codegen speedup.

<a name="ll-18"></a>
### [LL-18] [H] Slice patterns: the `..` rest marker is **lost** — first plain binding misinterpreted as the rest binder
**Where:** `crates/glyim-typeck/src/check_pat.rs:554-575` (root cause: glyim-hir/src/lower/lower_pat.rs:128-136)
`..` inside `PatSlice` is a bare token that lowering silently drops; `[a, b]` reaches typeck as two plain `Pat::Binding`s and this code treats the first as the rest binder — `a` gets bound with the whole array type. The corpus test `glyim-lower/tests/mir/slice_pattern.g` is marked `// ignore` — consistent with this never having worked.

**Fix:**
1. In lower_pat.rs:128-136, when a `DotDot` token is a direct child of `PatSlice`, push `Pat::Rest`:
```rust
for child in node.children_with_tokens() {
    match child {
        NodeOrToken::Token(t) if t.kind() == SyntaxKind::DotDot => {
            pats.push(Pat::Rest);   // add the variant (or Pat::Wild with a has_rest flag on Pat::Slice)
        }
        NodeOrToken::Node(n) => { …existing lower… }
        _ => {}
    }
}
```
2. In check_pat.rs:554-575, change `is_slice` to `matches!(sub, Pat::Rest)` only.

**Check:** un-ignore `slice_pattern.g`; `[a, .., c]` binds `a`/`c` as elements and skips the middle.

<a name="ll-19"></a>
### [LL-19] [H] Missing generic arguments silently filled with `Ty::ERROR` — failure surfaces much later as an opaque LLVM internal error
**Where:** `crates/glyim-typeck/src/check_expr.rs:1107-1122`
`unwrap_or_else(|| GenericArg::Ty(self.ctx.error_ty()))` — no diagnostic; the failure reappears as "Attempted to lower TyKind::Error" deep in the backend with no pointer to the user's mistake.

**Fix:**
```diff
                                         .or_else(|| base_substs.get(i).cloned())
-                                        .unwrap_or_else(|| GenericArg::Ty(self.ctx.error_ty()))
+                                        .unwrap_or_else(|| {
+                                            self.diagnostics.push(GlyimDiagnostic::type_error(
+                                                format!("type annotations needed: generic parameter \
+                                                         #{} of `{}` cannot be inferred", i, callee_name)));
+                                            GenericArg::Ty(self.ctx.error_ty())
+                                        })
```
**Check:** calling a generic fn with uninferrable params reports at the call site, not in codegen.

---

## 8. Cross-cutting verification plan

Run these after the corresponding sections, in order. Build without LLVM if needed: `cargo build --workspace --no-default-features` (bytecode path) — but the LL-* fixes need `LLVM_SYS_220_PREFIX` set.

1. **Lexer/parser (FE):** add the failing inputs as fixtures in `crates/glyim-frontend/src/tests/` — `x %= 2`, `(1,)`, `x as u8 as u32`, `Vec<u8>::new()`, `-3..=3`, `&&5`, `x @ 1..=5`, `continue 'a;`, `| 1 => …`, `pub(in crate::foo)`, `extern crate foo;`, `const _`, `0x`, `1e_`, unterminated `/*`. Each becomes a snapshot or parse-must-succeed test.
2. **Solver (SOLVE):** `cargo test -p glyim-solve -p glyim-typeck`. Add: `(a,b)~(b,a)` int-var cycle test; `impl<T: Foo> Foo for T {}` no-crash test; `&i32 → &str` non-coercible test.
3. **MIR/opts (MIR):** `cargo test -p glyim-opt -p glyim-mir-interp -p glyim-borrowck`. Add: stale-map const-prop body; DCE-with-Drop body; array-drop CFG edge assertion; `[String;3]` drop-count interp test; `arr[i]` alias borrowck test; call-arg use-after-move test.
4. **Bytecode contract (RT-3/4/5/11/12):** ONE end-to-end test: `generate()` → deserialize → `Vm::run` on a program with if/else, loop, call, enum match, `.len()` — compare against the interpreter. This single test would have caught 6 of the bytecode bugs.
5. **Runtime (RT-21/24/25):** async TCP echo test with a hard timeout; concurrent wait+kill test.
6. **LLVM backend (LL):** run-pass fixtures: unsigned compare, `i8 as i32` negative, `f as u64`, `u64 as f64`, i32-indexed array, out-of-order struct literal, ZST-arg fn, loop with struct literal, `&&` short-circuit with side effect.
7. **Full suite:** `cargo test --workspace` (4,186 tests must stay green — every fix above is designed to be behavior-fixing, not API-breaking; where a test pinned wrong behavior — e.g. `kind_tests.rs:97`, `array_drop_creates_loop` — update the test and note it in the commit).

## 9. Statistics

| Cluster | Bugs | Perf | Critical bugs |
|---------|-----:|-----:|--------------|
| Frontend (lexer/parser) [FE] | 19 | 1 | 1 (FE-6) |
| Solver/unify [SOLVE] | 19 | 4 | 3 (SOLVE-1/2/8) |
| MIR/opts/interp/borrowck [MIR] | 27 | 4 | 6 (MIR-1/6/10/11/17/21/24) |
| VM/codegen/runtime/CLI/layout [RT] | 28 | 5 | 7 (RT-3/4/11/12/13/21/31) |
| glyip/LSP/pipeline/pilot [INF] | 27 | 5 | 5 (INF-11/12/13/16/23) |
| HIR/meta/defmap/const-eval/lower [HIR] | 32 | 3 | 7 (HIR-1/2/3/4/10/11/29/30/31) |
| LLVM backend/typeck [LL] | 17 | 2 | 6 (LL-1/2/5/6/7/9/10/11) |
| **Total** | **172** | **21** | **35** |

The dominant themes, worth fixing as systems rather than patches:

1. **The bytecode contract is fiction.** Emitter, peephole decoder, and VM disagree on operand encodings (RT-3/4/5), branch polarity (RT-11), target addressing (RT-12), index strides (RT-13), and discriminants (RT-6). Fix as one PR with the end-to-end test from §8.4.
2. **A systematic signedness gap in the LLVM backend** (LL-1→LL-4): every unsigned compare, every widening cast, every float↔int conversion. One helper (`is_signed_int_ty(src_ty)`) fixes all four.
3. **Side-effect-unsafe transformations.** Const-prop's stale map (MIR-1), DCE vs Drop (MIR-6), drop-elaboration's CFG corruption (MIR-10), double scrutinee evaluation (HIR-29), discarded break values (HIR-30), by-value ref captures (HIR-31) — each violates "don't change observable behavior". Every optimizer pass needs a "no side effects dropped, no evaluation duplicated" property test.
4. **Unchecked probe mutation in inference** (SOLVE-4, LL-11, check_expr :3010, :3362): speculative unification without snapshot/rollback poisons the table. One `snapshot/rollback` discipline + the undo-log from SOLVE-21 fixes the class.
5. **Silent wrong answers instead of errors** throughout: `unwrap_or(0)` literals (HIR-15), `f32 == f32 → false` (HIR-23), `%0 → 0` (RT-7), empty env var → missing (RT-29), missing generics → `Ty::ERROR` (LL-19). Prefer a loud error over a wrong value everywhere.
