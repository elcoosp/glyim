# `format!("{}", x)` interpolation — root-cause diagnosis

The probe `probe_string_push_str.g` (`println!("{}", s.as_str())`) does not
work. Attempts to fix it at the emitter, the LLVM intrinsic layer, and the
pipeline all made partial progress but never landed. This documents *why*,
so the next attempt starts from the real causes instead of re-discovering
them.

## Two independent bugs block it

### 1. Method resolution fails on a parenthesized receiver

The `format!` expansion emits `__glyim_fmt.push_str((<arg>).to_string().as_str())`.
With the parens, `(<arg>).to_string()` typechecks to `Error` — confirmed by
`Assign(local14, Constant(Error))` in the MIR of `main`. Removing the parens
**fixes typecheck** (LLVM then proceeds past it), and standalone
`(s.as_str()).to_string().as_str()` compiles fine as source — so the failure
is specific to a parenthesized receiver inside an expander-produced stream,
not to the syntax itself. User macros re-emitting `$e` (test B) and nested
user macros (test C) both pass, so it is not general hygiene; it is
parenthesized-receiver method lookup on builtin-registered methods.

**Fix location:** typeck method resolution (`check_expr.rs`) — a
parenthesized expression should transparently forward method lookup to its
inner expression, including the builtin-method table.

### 2. `&mut self` receivers are lowered by value

`String::push_str` is `fn push_str(&mut self, s: &str)`. Its MIR call is:
